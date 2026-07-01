use std::error::Error;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use frigg::settings::FriggConfig;
use frigg::storage::resolve_workspace_relative_write_path;

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

    if plan.action_count(AdoptPlanAction::Error) > 0 {
        return Err(Box::new(io::Error::other(
            "adopt failed: plan contains target error(s)",
        )));
    }

    if dry_run {
        return Ok(());
    }

    let writes = apply_plan_entries(&plan, uninstall, force)?;
    if writes > 0 {
        println!("adopt apply writes={writes}");
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
        let root = config
            .root_by_repository_id(&repo.repository_id.0)
            .ok_or_else(|| {
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
    root: &Path,
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

fn apply_plan_entries(
    plan: &AdoptPlan,
    uninstall: bool,
    force: bool,
) -> Result<usize, Box<dyn Error>> {
    let mut writes = 0;

    for entry in &plan.entries {
        if !matches!(
            entry.action,
            AdoptPlanAction::Create | AdoptPlanAction::Update | AdoptPlanAction::Remove
        ) {
            continue;
        }

        let write_path = resolve_entry_write_path(entry)?;
        let contents = match fs::read_to_string(&entry.path) {
            Ok(contents) => Some(contents),
            Err(err) if err.kind() == io::ErrorKind::NotFound => None,
            Err(err) => return Err(Box::new(err)),
        };

        let edit = if matches!(
            entry.target,
            AdoptTarget::McpProject | AdoptTarget::McpCursor
        ) {
            apply_mcp_json_edit(contents.as_deref(), uninstall, force)
        } else {
            apply_markdown_edit(contents.as_deref(), uninstall)
        }?;

        match edit {
            AdoptApplyEdit::Write(updated) => {
                fs::write(&write_path, updated)?;
                writes += 1;
            }
            AdoptApplyEdit::Delete => {
                match fs::remove_file(&write_path) {
                    Ok(()) => {}
                    Err(err) if err.kind() == io::ErrorKind::NotFound => {}
                    Err(err) => return Err(Box::new(err)),
                }
                writes += 1;
            }
            AdoptApplyEdit::Unchanged => {}
        }
    }

    Ok(writes)
}

#[cfg(test)]
fn apply_mcp_json_entries(plan: &AdoptPlan, uninstall: bool, force: bool) -> io::Result<usize> {
    apply_plan_entries(plan, uninstall, force).map_err(|err| io::Error::other(err.to_string()))
}

fn resolve_entry_write_path(entry: &AdoptPlanEntry) -> Result<PathBuf, Box<dyn Error>> {
    resolve_workspace_relative_write_path(&entry.root, Path::new(entry.target.path()))
        .map_err(|err| Box::new(err) as Box<dyn Error>)
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum AdoptApplyEdit {
    Write(String),
    Delete,
    Unchanged,
}

fn apply_markdown_edit(
    contents: Option<&str>,
    uninstall: bool,
) -> Result<AdoptApplyEdit, Box<dyn Error>> {
    if uninstall {
        let Some(contents) = contents else {
            return Ok(AdoptApplyEdit::Unchanged);
        };
        return match managed_block::remove_managed_block(contents) {
            Ok(managed_block::ManagedBlockEdit::Changed(updated)) => {
                if updated.trim().is_empty() {
                    Ok(AdoptApplyEdit::Delete)
                } else {
                    Ok(AdoptApplyEdit::Write(updated))
                }
            }
            Ok(managed_block::ManagedBlockEdit::Unchanged) => Ok(AdoptApplyEdit::Unchanged),
            Err(err) => Err(Box::new(err)),
        };
    }

    let desired = managed_block::desired_markdown();
    let edit = managed_block::upsert_managed_block(contents.unwrap_or_default(), &desired)?;
    Ok(match edit {
        managed_block::ManagedBlockEdit::Changed(updated) => AdoptApplyEdit::Write(updated),
        managed_block::ManagedBlockEdit::Unchanged => AdoptApplyEdit::Unchanged,
    })
}

fn apply_mcp_json_edit(
    contents: Option<&str>,
    uninstall: bool,
    force: bool,
) -> Result<AdoptApplyEdit, Box<dyn Error>> {
    let edit = if uninstall {
        match contents {
            Some(contents) => json_merge::remove_mcp_server(contents, force),
            None => Ok(json_merge::McpJsonEdit::Unchanged),
        }
    } else {
        json_merge::upsert_mcp_server(contents, force)
    }?;

    Ok(match edit {
        json_merge::McpJsonEdit::Changed(updated) => AdoptApplyEdit::Write(updated),
        json_merge::McpJsonEdit::Unchanged | json_merge::McpJsonEdit::Skipped => {
            AdoptApplyEdit::Unchanged
        }
    })
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

    use super::{apply_mcp_json_entries, apply_plan_entries, build_adopt_plan};
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

    #[test]
    fn adopt_apply_creates_missing_mcp_config() {
        let root = temp_dir("adopt-apply-create-mcp");
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

        assert_eq!(
            apply_mcp_json_entries(&plan, false, false).expect("apply mcp"),
            1
        );
        let value: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(root.join(".mcp.json")).expect("read mcp"))
                .expect("parse mcp");
        assert_eq!(
            value["mcpServers"]["frigg"],
            super::json_merge::desired_mcp_server()
        );
        fs::remove_dir_all(root).expect("remove temp root");
    }

    #[test]
    fn adopt_apply_creates_missing_markdown_with_managed_block() {
        let root = temp_dir("adopt-apply-create-markdown");
        fs::create_dir_all(&root).expect("create temp root");
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

        assert_eq!(
            apply_plan_entries(&plan, false, false).expect("apply markdown"),
            1
        );
        assert_eq!(
            fs::read_to_string(root.join("AGENTS.md")).expect("read agents"),
            super::managed_block::desired_markdown()
        );
        fs::remove_dir_all(root).expect("remove temp root");
    }

    #[test]
    fn adopt_apply_removes_owned_markdown_file_on_uninstall() {
        let root = temp_dir("adopt-apply-remove-owned-markdown");
        fs::create_dir_all(&root).expect("create temp root");
        fs::write(
            root.join("AGENTS.md"),
            super::managed_block::desired_markdown(),
        )
        .expect("write agents");
        let config = FriggConfig::from_workspace_roots(vec![root.clone()])
            .expect("config should accept workspace root");
        let plan = build_adopt_plan(&config, &[AdoptTarget::AgentsMd], false, false, true, false)
            .expect("build adopt plan");

        assert_eq!(
            apply_plan_entries(&plan, true, false).expect("apply uninstall"),
            1
        );
        assert!(!root.join("AGENTS.md").exists());
        fs::remove_dir_all(root).expect("remove temp root");
    }

    #[test]
    fn adopt_apply_preserves_sibling_mcp_servers() {
        let root = temp_dir("adopt-apply-preserve-mcp");
        fs::create_dir_all(&root).expect("create temp root");
        fs::write(
            root.join(".mcp.json"),
            r#"{"mcpServers":{"other":{"command":"other"}},"unrelated":true}"#,
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

        assert_eq!(
            apply_mcp_json_entries(&plan, false, false).expect("apply mcp"),
            1
        );
        let value: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(root.join(".mcp.json")).expect("read mcp"))
                .expect("parse mcp");
        assert_eq!(value["mcpServers"]["other"]["command"], "other");
        assert_eq!(value["unrelated"], true);
        assert_eq!(
            value["mcpServers"]["frigg"],
            super::json_merge::desired_mcp_server()
        );
        fs::remove_dir_all(root).expect("remove temp root");
    }

    #[test]
    fn adopt_apply_removes_only_frigg_mcp_server_on_uninstall() {
        let root = temp_dir("adopt-apply-remove-mcp");
        fs::create_dir_all(&root).expect("create temp root");
        fs::write(
            root.join(".mcp.json"),
            r#"{"mcpServers":{"frigg":{"type":"http","url":"http://127.0.0.1:37444/mcp"},"other":{"command":"other"}},"unrelated":true}"#,
        )
        .expect("write mcp");
        let config = FriggConfig::from_workspace_roots(vec![root.clone()])
            .expect("config should accept workspace root");
        let plan = build_adopt_plan(
            &config,
            &[AdoptTarget::McpProject],
            false,
            false,
            true,
            false,
        )
        .expect("build adopt plan");

        assert_eq!(
            apply_mcp_json_entries(&plan, true, false).expect("apply mcp"),
            1
        );
        let value: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(root.join(".mcp.json")).expect("read mcp"))
                .expect("parse mcp");
        assert!(value["mcpServers"].get("frigg").is_none());
        assert_eq!(value["mcpServers"]["other"]["command"], "other");
        assert_eq!(value["unrelated"], true);
        fs::remove_dir_all(root).expect("remove temp root");
    }

    #[cfg(unix)]
    #[test]
    fn adopt_apply_rejects_symlink_parent_escape_before_write() {
        let root = temp_dir("adopt-apply-symlink-root");
        let outside = temp_dir("adopt-apply-symlink-outside");
        fs::create_dir_all(&root).expect("create temp root");
        fs::create_dir_all(&outside).expect("create outside root");
        std::os::unix::fs::symlink(&outside, root.join(".cursor")).expect("create cursor symlink");
        let config = FriggConfig::from_workspace_roots(vec![root.clone()])
            .expect("config should accept workspace root");
        let plan = build_adopt_plan(&config, &[AdoptTarget::Cursor], false, false, false, false)
            .expect("build adopt plan");

        let err = apply_plan_entries(&plan, false, false)
            .expect_err("symlinked parent escape should fail");

        assert!(err.to_string().contains("escapes canonical workspace root"));
        assert!(!outside.join("rules/frigg.mdc").exists());
        fs::remove_dir_all(root).expect("remove temp root");
        fs::remove_dir_all(outside).expect("remove outside root");
    }

    fn temp_dir(stem: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("{stem}-{unique}"))
    }
}
