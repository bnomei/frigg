use std::error::Error;
use std::fs;
use std::io;

use frigg::settings::FriggConfig;

use crate::cli_args::AdoptTarget;

mod json_merge;
mod managed_block;
mod plan;
mod targets;

use plan::{AdoptPlan, AdoptPlanAction, AdoptPlanEntry};
use targets::select_targets;

#[allow(clippy::too_many_arguments)]
pub(crate) fn run_adopt_command(
    config: &FriggConfig,
    requested_targets: Vec<AdoptTarget>,
    all: bool,
    legacy_cursor: bool,
    uninstall: bool,
    check: bool,
    dry_run: bool,
    force: bool,
) -> Result<(), Box<dyn Error>> {
    let plan = build_adopt_plan(
        config,
        &requested_targets,
        all,
        legacy_cursor,
        uninstall,
        force,
    )?;
    let pending_changes = plan.pending_changes();
    let status = if plan.is_empty() {
        "noop"
    } else if check && pending_changes > 0 {
        "pending"
    } else {
        "planned"
    };

    println!(
        "adopt summary status={} repositories={} targets={} create={} update={} unchanged={} remove={} skipped={} error={} pending={} dry_run={} check={} uninstall={} force={} writes=0",
        status,
        config.repositories().len(),
        plan.len(),
        plan.action_count(AdoptPlanAction::Create),
        plan.action_count(AdoptPlanAction::Update),
        plan.action_count(AdoptPlanAction::Unchanged),
        plan.action_count(AdoptPlanAction::Remove),
        plan.action_count(AdoptPlanAction::Skipped),
        plan.action_count(AdoptPlanAction::Error),
        pending_changes,
        dry_run,
        check,
        uninstall,
        force
    );

    for entry in &plan.entries {
        println!(
            "adopt plan repository_id={} root={} target={:?} path={} action={} reason={} writes=0",
            entry.repository_id,
            entry.root.display(),
            entry.target,
            entry.path.display(),
            entry.action.as_str(),
            entry.reason.as_deref().unwrap_or("-")
        );
    }

    if check && pending_changes > 0 {
        return Err(Box::new(io::Error::other(format!(
            "adopt check failed: {pending_changes} pending change(s)"
        ))));
    }

    Ok(())
}

fn build_adopt_plan(
    config: &FriggConfig,
    requested_targets: &[AdoptTarget],
    all: bool,
    legacy_cursor: bool,
    uninstall: bool,
    force: bool,
) -> io::Result<AdoptPlan> {
    let repositories = config.repositories();
    let mut entries = Vec::new();

    for repo in &repositories {
        let root = config.root_by_repository_id(&repo.repository_id.0).ok_or_else(|| {
            io::Error::other(format!(
                "adopt summary status=failed repository_id={} error=workspace root lookup failed",
                repo.repository_id.0
            ))
        })?;

        for target in select_targets(root, requested_targets, all, legacy_cursor) {
            let (action, reason) = classify_target_action(root, target, uninstall, force);
            entries.push(AdoptPlanEntry {
                repository_id: repo.repository_id.0.clone(),
                root: root.to_path_buf(),
                target,
                path: root.join(target.path()),
                action,
                reason,
            });
        }
    }

    Ok(AdoptPlan::new(entries))
}

fn classify_target_action(
    root: &std::path::Path,
    target: AdoptTarget,
    uninstall: bool,
    force: bool,
) -> (AdoptPlanAction, Option<String>) {
    let path = root.join(target.path());
    if !path.exists() {
        return if uninstall {
            (
                AdoptPlanAction::Unchanged,
                Some("target-missing".to_owned()),
            )
        } else {
            (AdoptPlanAction::Create, Some("target-missing".to_owned()))
        };
    };
    let contents = match fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(err) => return (AdoptPlanAction::Error, Some(format!("read-error:{err}"))),
    };

    if matches!(target, AdoptTarget::McpProject | AdoptTarget::McpCursor) {
        classify_mcp_target(&contents, uninstall, force)
    } else {
        classify_markdown_target(&contents, uninstall)
    }
}

fn classify_markdown_target(contents: &str, uninstall: bool) -> (AdoptPlanAction, Option<String>) {
    let desired = managed_block::desired_markdown();

    if uninstall {
        return match managed_block::remove_managed_block(contents) {
            Ok(managed_block::ManagedBlockEdit::Changed(_)) => (
                AdoptPlanAction::Remove,
                Some("managed-block-present".to_owned()),
            ),
            Ok(managed_block::ManagedBlockEdit::Unchanged) => (
                AdoptPlanAction::Unchanged,
                Some("managed-block-absent".to_owned()),
            ),
            Err(err) => (
                AdoptPlanAction::Error,
                Some(format!("invalid-managed-block:{err}")),
            ),
        };
    }

    match managed_block::upsert_managed_block(contents, &desired) {
        Ok(managed_block::ManagedBlockEdit::Unchanged) => (
            AdoptPlanAction::Unchanged,
            Some("managed-block-current".to_owned()),
        ),
        Ok(managed_block::ManagedBlockEdit::Changed(_)) => {
            if managed_block::has_managed_block(contents) {
                (
                    AdoptPlanAction::Update,
                    Some("managed-block-drifted".to_owned()),
                )
            } else {
                (
                    AdoptPlanAction::Update,
                    Some("managed-block-absent".to_owned()),
                )
            }
        }
        Err(err) => (
            AdoptPlanAction::Error,
            Some(format!("invalid-managed-block:{err}")),
        ),
    }
}

fn classify_mcp_target(
    contents: &str,
    uninstall: bool,
    force: bool,
) -> (AdoptPlanAction, Option<String>) {
    let state = match json_merge::classify_mcp_entry(contents) {
        Ok(state) => state,
        Err(err) => {
            return (AdoptPlanAction::Error, Some(format!("invalid-json:{err}")));
        }
    };

    match (uninstall, state, force) {
        (true, json_merge::McpEntryState::Desired, _) => (
            AdoptPlanAction::Remove,
            Some("frigg-entry-present".to_owned()),
        ),
        (true, json_merge::McpEntryState::Diverged, false) => (
            AdoptPlanAction::Skipped,
            Some("frigg-entry-diverged".to_owned()),
        ),
        (true, json_merge::McpEntryState::Diverged, true) => (
            AdoptPlanAction::Remove,
            Some("force-diverged-frigg-entry".to_owned()),
        ),
        (true, json_merge::McpEntryState::Missing, _) => (
            AdoptPlanAction::Unchanged,
            Some("frigg-entry-absent".to_owned()),
        ),
        (false, json_merge::McpEntryState::Desired, _) => (
            AdoptPlanAction::Unchanged,
            Some("frigg-entry-current".to_owned()),
        ),
        (false, json_merge::McpEntryState::Missing, _) => (
            AdoptPlanAction::Update,
            Some("frigg-entry-absent".to_owned()),
        ),
        (false, json_merge::McpEntryState::Diverged, false) => (
            AdoptPlanAction::Skipped,
            Some("frigg-entry-diverged".to_owned()),
        ),
        (false, json_merge::McpEntryState::Diverged, true) => (
            AdoptPlanAction::Update,
            Some("force-diverged-frigg-entry".to_owned()),
        ),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use frigg::settings::FriggConfig;

    use super::build_adopt_plan;
    use crate::cli_args::AdoptTarget;

    #[test]
    fn adopt_plan_uses_workspace_roots_and_targets() {
        let root = temp_dir("adopt-plan");
        fs::create_dir_all(&root).expect("create temp root");
        let config = FriggConfig::from_workspace_roots(vec![root.clone()])
            .expect("config should accept workspace root");

        let plan = build_adopt_plan(
            &config,
            &[AdoptTarget::McpProject],
            false,
            false,
            false,
            false,
        )
        .expect("build adopt plan");

        assert_eq!(plan.len(), 1);
        assert_eq!(plan.entries[0].repository_id, "repo-001");
        assert_eq!(plan.entries[0].path, root.join(".mcp.json"));
        assert_eq!(plan.entries[0].action, super::AdoptPlanAction::Create);
        fs::remove_dir_all(root).expect("remove temp root");
    }

    #[test]
    fn adopt_plan_reports_unchanged_markdown_when_block_is_current() {
        let root = temp_dir("adopt-plan-current-markdown");
        fs::create_dir_all(&root).expect("create temp root");
        fs::write(
            root.join("AGENTS.md"),
            super::managed_block::desired_markdown(),
        )
        .expect("write agents");
        let config = FriggConfig::from_workspace_roots(vec![root.clone()])
            .expect("config should accept workspace root");

        let plan = build_adopt_plan(
            &config,
            &[AdoptTarget::AgentsMd],
            false,
            false,
            false,
            false,
        )
        .expect("build adopt plan");

        assert_eq!(plan.entries[0].action, super::AdoptPlanAction::Unchanged);
        fs::remove_dir_all(root).expect("remove temp root");
    }

    #[test]
    fn adopt_plan_skips_diverged_mcp_without_force() {
        let root = temp_dir("adopt-plan-diverged-mcp");
        fs::create_dir_all(&root).expect("create temp root");
        fs::write(
            root.join(".mcp.json"),
            r#"{"mcpServers":{"frigg":{"command":"frigg"}}}"#,
        )
        .expect("write mcp");
        let config = FriggConfig::from_workspace_roots(vec![root.clone()])
            .expect("config should accept workspace root");

        let plan = build_adopt_plan(
            &config,
            &[AdoptTarget::McpProject],
            false,
            false,
            false,
            false,
        )
        .expect("build adopt plan");

        assert_eq!(plan.entries[0].action, super::AdoptPlanAction::Skipped);
        fs::remove_dir_all(root).expect("remove temp root");
    }

    fn temp_dir(stem: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("{stem}-{unique}"))
    }
}
