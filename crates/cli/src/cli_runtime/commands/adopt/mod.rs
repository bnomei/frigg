//! CLI `adopt` command: plans and applies Frigg-managed entries across agent docs, MCP JSON, and Claude hooks.
//!
//! Supports dry-run, check, uninstall, and force modes while preserving sibling config content and
//! rejecting symlink escapes before writes.

use std::error::Error;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

use frigg::settings::FriggConfig;
use frigg::storage::resolve_workspace_relative_write_path;

use crate::cli_args::AdoptTarget;
use crate::cli_runtime::{
    CliOutput, OutputField, OutputLevel, field, format_output_event_line, reported_error,
};

mod json_merge;
mod managed_block;
mod plan;
mod targets;

use plan::{AdoptPlan, AdoptPlanAction, AdoptPlanEntry};
use targets::select_targets;

/// Plans adopt work for every configured repository, then applies or reports changes through `CliOutput`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn run_adopt_command_with_output(
    config: &FriggConfig,
    requested_targets: Vec<AdoptTarget>,
    all: bool,
    uninstall: bool,
    check: bool,
    dry_run: bool,
    force: bool,
    mcp_server_url: &str,
    output: &CliOutput,
) -> Result<(), Box<dyn Error>> {
    output.progress_event(
        OutputLevel::Info,
        "adopt",
        "start",
        &[
            field("status", "starting"),
            field("repos", config.repositories().len()),
            field("requested_targets", requested_targets.len()),
            field("all", all),
            field("dry_run", dry_run),
            field("check", check),
            field("uninstall", uninstall),
            field("force", force),
        ],
        None,
    )?;

    let plan = build_adopt_plan(
        config,
        &requested_targets,
        all,
        uninstall,
        force,
        mcp_server_url,
    )?;
    let pending_changes = plan.pending_changes();
    let status = if plan.is_empty() {
        "noop"
    } else if check && pending_changes > 0 {
        "pending"
    } else {
        "planned"
    };

    if dry_run || check {
        output.result_event(
            adopt_level_for_status(status),
            "adopt",
            "complete",
            &adopt_summary_fields(
                status,
                config.repositories().len(),
                &plan,
                pending_changes,
                dry_run,
                check,
                uninstall,
                force,
                0,
            ),
            None,
        )?;
        for entry in &plan.entries {
            output.result_event(
                adopt_level_for_action(entry.action),
                "adopt",
                "plan",
                &adopt_plan_fields(entry),
                Some(&entry.path.display().to_string()),
            )?;
        }
    }

    if check && pending_changes > 0 {
        let message = format!("adopt check failed: {pending_changes} pending change(s)");
        output.error_event(
            "adopt",
            "failed",
            &[
                field("status", "failed"),
                field("mode", "check"),
                field("pending", pending_changes),
                field("error", &message),
            ],
            None,
        )?;
        return Err(reported_error(message));
    }

    if plan.action_count(AdoptPlanAction::Error) > 0 {
        let message = "adopt failed: plan contains target error(s)";
        output.error_event(
            "adopt",
            "failed",
            &[
                field("status", "failed"),
                field("targets", plan.len()),
                field("error", message),
            ],
            None,
        )?;
        return Err(reported_error(message));
    }

    if dry_run || check {
        return Ok(());
    }

    for entry in &plan.entries {
        output.progress_event(
            adopt_level_for_action(entry.action),
            "adopt",
            "plan",
            &adopt_plan_fields(entry),
            Some(&entry.path.display().to_string()),
        )?;
    }

    let writes = apply_plan_entries(&plan, uninstall, force, mcp_server_url)?;
    output.summary_event(
        adopt_level_for_status(status),
        "adopt",
        "complete",
        &adopt_summary_fields(
            status,
            config.repositories().len(),
            &plan,
            pending_changes,
            dry_run,
            check,
            uninstall,
            force,
            writes,
        ),
        None,
    )?;

    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn adopt_summary_fields(
    status: &str,
    repositories: usize,
    plan: &AdoptPlan,
    pending_changes: usize,
    dry_run: bool,
    check: bool,
    uninstall: bool,
    force: bool,
    writes: usize,
) -> Vec<OutputField> {
    vec![
        field("status", status),
        field("repos", repositories),
        field("targets", plan.len()),
        field("create", plan.action_count(AdoptPlanAction::Create)),
        field("update", plan.action_count(AdoptPlanAction::Update)),
        field("unchanged", plan.action_count(AdoptPlanAction::Unchanged)),
        field("remove", plan.action_count(AdoptPlanAction::Remove)),
        field("skipped", plan.action_count(AdoptPlanAction::Skipped)),
        field("error", plan.action_count(AdoptPlanAction::Error)),
        field("pending", pending_changes),
        field("dry_run", dry_run),
        field("check", check),
        field("uninstall", uninstall),
        field("force", force),
        field("writes", writes),
    ]
}

fn adopt_plan_fields(entry: &AdoptPlanEntry) -> Vec<OutputField> {
    vec![
        field("repo", &entry.repository_id),
        field("target", format!("{:?}", entry.target)),
        field("action", entry.action.as_str()),
        field("reason", entry.reason.as_deref().unwrap_or("-")),
        field("writes", 0),
        field("root", entry.root.display()),
    ]
}

fn adopt_level_for_status(status: &str) -> OutputLevel {
    match status {
        "pending" => OutputLevel::Warn,
        "noop" => OutputLevel::Skip,
        _ => OutputLevel::Ok,
    }
}

fn adopt_level_for_action(action: AdoptPlanAction) -> OutputLevel {
    match action {
        AdoptPlanAction::Create | AdoptPlanAction::Update | AdoptPlanAction::Remove => {
            OutputLevel::Info
        }
        AdoptPlanAction::Unchanged | AdoptPlanAction::Skipped => OutputLevel::Skip,
        AdoptPlanAction::Error => OutputLevel::Error,
    }
}

fn build_adopt_plan(
    config: &FriggConfig,
    requested_targets: &[AdoptTarget],
    all: bool,
    uninstall: bool,
    force: bool,
    mcp_server_url: &str,
) -> io::Result<AdoptPlan> {
    let repositories = config.repositories();
    let mut entries = Vec::new();

    for repo in &repositories {
        let root = config
            .root_by_repository_id(&repo.repository_id.0)
            .ok_or_else(|| {
                io::Error::other(format_output_event_line(
                    OutputLevel::Error,
                    "adopt",
                    "failed",
                    &[
                        field("status", "failed"),
                        field("repo", &repo.repository_id.0),
                        field("error", "workspace root lookup failed"),
                    ],
                    None,
                ))
            })?;

        for target in select_targets(root, requested_targets, all) {
            let (action, reason) =
                classify_target_action(root, target, uninstall, force, mcp_server_url);
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
    mcp_server_url: &str,
) -> (AdoptPlanAction, Option<String>) {
    let contents = match read_existing_target_contents(root, target) {
        Ok(Some(contents)) => contents,
        Ok(None) => {
            return classify_missing_target(uninstall);
        }
        Err(err) => return (AdoptPlanAction::Error, Some(format!("read-error:{err}"))),
    };

    if matches!(target, AdoptTarget::Hook) {
        classify_claude_hook_target(&contents, uninstall)
    } else if matches!(target, AdoptTarget::McpProject | AdoptTarget::McpCursor) {
        classify_mcp_target(&contents, uninstall, force, mcp_server_url)
    } else {
        classify_markdown_target(&contents, uninstall)
    }
}

fn read_existing_target_contents(root: &Path, target: AdoptTarget) -> io::Result<Option<String>> {
    read_existing_workspace_relative_file(root, Path::new(target.path()))
}

fn read_existing_workspace_relative_file(
    root: &Path,
    relative_path: &Path,
) -> io::Result<Option<String>> {
    validate_adopt_target_relative_path(relative_path)?;
    let root_canonical = root.canonicalize().map_err(|err| {
        io::Error::other(format!(
            "failed to canonicalize workspace root {}: {err}",
            root.display()
        ))
    })?;
    let path = root_canonical.join(relative_path);
    if !path.try_exists()? {
        return Ok(None);
    }

    let canonical_path = path.canonicalize().map_err(|err| {
        io::Error::other(format!(
            "failed to canonicalize adopt target read path {}: {err}",
            path.display()
        ))
    })?;
    if !canonical_path.starts_with(&root_canonical) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "adopt target read path escapes canonical workspace root boundary: {}",
                path.display()
            ),
        ));
    }

    fs::read_to_string(canonical_path).map(Some)
}

fn validate_adopt_target_relative_path(relative_path: &Path) -> io::Result<()> {
    if relative_path.as_os_str().is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "adopt target read path must not be empty",
        ));
    }

    for component in relative_path.components() {
        match component {
            Component::Normal(_) | Component::CurDir => {}
            Component::ParentDir => {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    format!(
                        "adopt target read path contains parent traversal: {}",
                        relative_path.display()
                    ),
                ));
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    format!(
                        "adopt target read path must be relative: {}",
                        relative_path.display()
                    ),
                ));
            }
        }
    }

    Ok(())
}

fn classify_missing_target(uninstall: bool) -> (AdoptPlanAction, Option<String>) {
    if uninstall {
        (
            AdoptPlanAction::Unchanged,
            Some("target-missing".to_owned()),
        )
    } else {
        (AdoptPlanAction::Create, Some("target-missing".to_owned()))
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
    mcp_server_url: &str,
) -> Result<usize, Box<dyn Error>> {
    let mut writes = 0;

    for entry in &plan.entries {
        if !matches!(
            entry.action,
            AdoptPlanAction::Create | AdoptPlanAction::Update | AdoptPlanAction::Remove
        ) {
            continue;
        }

        let contents = read_existing_target_contents(&entry.root, entry.target)?;
        let write_path = resolve_entry_write_path(entry)?;

        let edit = if matches!(entry.target, AdoptTarget::Hook) {
            apply_claude_hook_edit(contents.as_deref(), uninstall)
        } else if matches!(
            entry.target,
            AdoptTarget::McpProject | AdoptTarget::McpCursor
        ) {
            apply_mcp_json_edit(contents.as_deref(), uninstall, force, mcp_server_url)
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
fn apply_mcp_json_entries(
    plan: &AdoptPlan,
    uninstall: bool,
    force: bool,
    mcp_server_url: &str,
) -> io::Result<usize> {
    apply_plan_entries(plan, uninstall, force, mcp_server_url)
        .map_err(|err| io::Error::other(err.to_string()))
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
    mcp_server_url: &str,
) -> Result<AdoptApplyEdit, Box<dyn Error>> {
    let edit = if uninstall {
        match contents {
            Some(contents) => json_merge::remove_mcp_server(contents, force, mcp_server_url),
            None => Ok(json_merge::McpJsonEdit::Unchanged),
        }
    } else {
        json_merge::upsert_mcp_server(contents, force, mcp_server_url)
    }?;

    Ok(match edit {
        json_merge::McpJsonEdit::Changed(updated) => AdoptApplyEdit::Write(updated),
        json_merge::McpJsonEdit::Unchanged | json_merge::McpJsonEdit::Skipped => {
            AdoptApplyEdit::Unchanged
        }
    })
}

fn apply_claude_hook_edit(
    contents: Option<&str>,
    uninstall: bool,
) -> Result<AdoptApplyEdit, Box<dyn Error>> {
    let edit = if uninstall {
        match contents {
            Some(contents) => json_merge::remove_claude_hook(contents),
            None => Ok(json_merge::McpJsonEdit::Unchanged),
        }
    } else {
        json_merge::upsert_claude_hook(contents)
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
    mcp_server_url: &str,
) -> (AdoptPlanAction, Option<String>) {
    let state = match if uninstall {
        json_merge::classify_mcp_entry_for_uninstall(contents)
    } else {
        json_merge::classify_mcp_entry(contents, mcp_server_url)
    } {
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

fn classify_claude_hook_target(
    contents: &str,
    uninstall: bool,
) -> (AdoptPlanAction, Option<String>) {
    let state = match json_merge::classify_claude_hook(contents) {
        Ok(state) => state,
        Err(err) => return (AdoptPlanAction::Error, Some(format!("invalid-json:{err}"))),
    };

    match (uninstall, state) {
        (true, json_merge::ClaudeHookState::Desired) => (
            AdoptPlanAction::Remove,
            Some("frigg-hook-present".to_owned()),
        ),
        (true, json_merge::ClaudeHookState::Diverged) => (
            AdoptPlanAction::Remove,
            Some("frigg-hook-diverged".to_owned()),
        ),
        (true, json_merge::ClaudeHookState::Missing) => (
            AdoptPlanAction::Unchanged,
            Some("frigg-hook-absent".to_owned()),
        ),
        (false, json_merge::ClaudeHookState::Desired) => (
            AdoptPlanAction::Unchanged,
            Some("frigg-hook-current".to_owned()),
        ),
        (false, json_merge::ClaudeHookState::Diverged) => (
            AdoptPlanAction::Update,
            Some("frigg-hook-diverged".to_owned()),
        ),
        (false, json_merge::ClaudeHookState::Missing) => (
            AdoptPlanAction::Update,
            Some("frigg-hook-absent".to_owned()),
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

    const TEST_MCP_SERVER_URL: &str = super::json_merge::DEFAULT_MCP_SERVER_URL;

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
            TEST_MCP_SERVER_URL,
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
            TEST_MCP_SERVER_URL,
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
            TEST_MCP_SERVER_URL,
        )
        .expect("build adopt plan");

        assert_eq!(plan.entries[0].action, super::AdoptPlanAction::Skipped);
        fs::remove_dir_all(root).expect("remove temp root");
    }

    #[test]
    fn adopt_plan_uninstall_removes_custom_port_mcp_entry() {
        let root = temp_dir("adopt-plan-uninstall-custom-port-mcp");
        fs::create_dir_all(&root).expect("create temp root");
        fs::write(
            root.join(".mcp.json"),
            r#"{"mcpServers":{"frigg":{"type":"http","url":"http://127.0.0.1:5000/mcp"}}}"#,
        )
        .expect("write mcp");
        let config = FriggConfig::from_workspace_roots(vec![root.clone()])
            .expect("config should accept workspace root");

        let plan = build_adopt_plan(
            &config,
            &[AdoptTarget::McpProject],
            false,
            true,
            false,
            TEST_MCP_SERVER_URL,
        )
        .expect("build adopt plan");

        assert_eq!(plan.entries[0].action, super::AdoptPlanAction::Remove);
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
            TEST_MCP_SERVER_URL,
        )
        .expect("build adopt plan");

        assert_eq!(
            apply_mcp_json_entries(&plan, false, false, TEST_MCP_SERVER_URL).expect("apply mcp"),
            1
        );
        let value: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(root.join(".mcp.json")).expect("read mcp"))
                .expect("parse mcp");
        assert_eq!(
            value["mcpServers"]["frigg"],
            super::json_merge::desired_mcp_server(TEST_MCP_SERVER_URL)
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
            TEST_MCP_SERVER_URL,
        )
        .expect("build adopt plan");

        assert_eq!(
            apply_plan_entries(&plan, false, false, TEST_MCP_SERVER_URL).expect("apply markdown"),
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
        let plan = build_adopt_plan(
            &config,
            &[AdoptTarget::AgentsMd],
            false,
            true,
            false,
            TEST_MCP_SERVER_URL,
        )
        .expect("build adopt plan");

        assert_eq!(
            apply_plan_entries(&plan, true, false, TEST_MCP_SERVER_URL).expect("apply uninstall"),
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
            TEST_MCP_SERVER_URL,
        )
        .expect("build adopt plan");

        assert_eq!(
            apply_mcp_json_entries(&plan, false, false, TEST_MCP_SERVER_URL).expect("apply mcp"),
            1
        );
        let value: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(root.join(".mcp.json")).expect("read mcp"))
                .expect("parse mcp");
        assert_eq!(value["mcpServers"]["other"]["command"], "other");
        assert_eq!(value["unrelated"], true);
        assert_eq!(
            value["mcpServers"]["frigg"],
            super::json_merge::desired_mcp_server(TEST_MCP_SERVER_URL)
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
            true,
            false,
            TEST_MCP_SERVER_URL,
        )
        .expect("build adopt plan");

        assert_eq!(
            apply_mcp_json_entries(&plan, true, false, TEST_MCP_SERVER_URL).expect("apply mcp"),
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

    #[test]
    fn adopt_apply_removes_custom_port_mcp_server_on_uninstall() {
        let root = temp_dir("adopt-apply-remove-custom-port-mcp");
        fs::create_dir_all(&root).expect("create temp root");
        fs::write(
            root.join(".mcp.json"),
            r#"{"mcpServers":{"frigg":{"type":"http","url":"http://127.0.0.1:5000/mcp"},"other":{"command":"other"}}}"#,
        )
        .expect("write mcp");
        let config = FriggConfig::from_workspace_roots(vec![root.clone()])
            .expect("config should accept workspace root");
        let plan = build_adopt_plan(
            &config,
            &[AdoptTarget::McpProject],
            false,
            true,
            false,
            TEST_MCP_SERVER_URL,
        )
        .expect("build adopt plan");

        assert_eq!(
            apply_mcp_json_entries(&plan, true, false, TEST_MCP_SERVER_URL).expect("apply mcp"),
            1
        );
        let value: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(root.join(".mcp.json")).expect("read mcp"))
                .expect("parse mcp");
        assert!(value["mcpServers"].get("frigg").is_none());
        assert_eq!(value["mcpServers"]["other"]["command"], "other");
        fs::remove_dir_all(root).expect("remove temp root");
    }

    #[test]
    fn adopt_apply_replaces_diverged_claude_hook_without_duplicating() {
        let root = temp_dir("adopt-apply-replace-diverged-hook");
        fs::create_dir_all(root.join(".claude")).expect("create claude dir");
        fs::write(
            root.join(".claude/settings.json"),
            r#"{"hooks":{"PreToolUse":[{"matcher":"Grep|Bash|Read","hooks":[{"type":"command","command":"frigg hook pretooluse","timeout":10}]}]}}"#,
        )
        .expect("write settings");
        let config = FriggConfig::from_workspace_roots(vec![root.clone()])
            .expect("config should accept workspace root");

        let plan = build_adopt_plan(
            &config,
            &[AdoptTarget::Hook],
            false,
            false,
            false,
            TEST_MCP_SERVER_URL,
        )
        .expect("build adopt plan");
        assert_eq!(plan.entries[0].action, super::AdoptPlanAction::Update);
        assert_eq!(
            plan.entries[0].reason.as_deref(),
            Some("frigg-hook-diverged")
        );
        assert_eq!(
            apply_plan_entries(&plan, false, false, TEST_MCP_SERVER_URL).expect("apply hook"),
            1
        );

        let value: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(root.join(".claude/settings.json")).expect("read settings"),
        )
        .expect("parse settings");
        let hooks = value["hooks"]["PreToolUse"][0]["hooks"]
            .as_array()
            .expect("hook array");
        assert_eq!(hooks.len(), 1);
        assert_eq!(hooks[0], super::json_merge::desired_claude_hook_command());

        let plan = build_adopt_plan(
            &config,
            &[AdoptTarget::Hook],
            false,
            false,
            false,
            TEST_MCP_SERVER_URL,
        )
        .expect("build adopt plan again");
        assert_eq!(plan.entries[0].action, super::AdoptPlanAction::Unchanged);
        fs::remove_dir_all(root).expect("remove temp root");
    }

    #[test]
    fn adopt_apply_deduplicates_mixed_current_and_diverged_claude_hooks() {
        let root = temp_dir("adopt-apply-deduplicate-mixed-hook");
        fs::create_dir_all(root.join(".claude")).expect("create claude dir");
        fs::write(
            root.join(".claude/settings.json"),
            r#"{"hooks":{"PreToolUse":[{"matcher":"Grep|Bash|Read","hooks":[{"type":"command","command":"frigg hook pretooluse","timeout":5},{"type":"command","command":"frigg hook pretooluse","timeout":10}]}]}}"#,
        )
        .expect("write settings");
        let config = FriggConfig::from_workspace_roots(vec![root.clone()])
            .expect("config should accept workspace root");

        let plan = build_adopt_plan(
            &config,
            &[AdoptTarget::Hook],
            false,
            false,
            false,
            TEST_MCP_SERVER_URL,
        )
        .expect("build adopt plan");
        assert_eq!(plan.entries[0].action, super::AdoptPlanAction::Update);
        assert_eq!(
            plan.entries[0].reason.as_deref(),
            Some("frigg-hook-diverged")
        );
        assert_eq!(
            apply_plan_entries(&plan, false, false, TEST_MCP_SERVER_URL).expect("apply hook"),
            1
        );

        let value: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(root.join(".claude/settings.json")).expect("read settings"),
        )
        .expect("parse settings");
        let hooks = value["hooks"]["PreToolUse"][0]["hooks"]
            .as_array()
            .expect("hook array");
        assert_eq!(hooks.len(), 1);
        assert_eq!(hooks[0], super::json_merge::desired_claude_hook_command());

        let plan = build_adopt_plan(
            &config,
            &[AdoptTarget::Hook],
            false,
            false,
            false,
            TEST_MCP_SERVER_URL,
        )
        .expect("build adopt plan again");
        assert_eq!(plan.entries[0].action, super::AdoptPlanAction::Unchanged);
        fs::remove_dir_all(root).expect("remove temp root");
    }

    #[test]
    fn adopt_apply_installs_claude_hook_idempotently() {
        let root = temp_dir("adopt-apply-install-hook");
        fs::create_dir_all(&root).expect("create temp root");
        let config = FriggConfig::from_workspace_roots(vec![root.clone()])
            .expect("config should accept workspace root");

        let plan = build_adopt_plan(
            &config,
            &[AdoptTarget::Hook],
            false,
            false,
            false,
            TEST_MCP_SERVER_URL,
        )
        .expect("build adopt plan");
        assert_eq!(plan.entries[0].action, super::AdoptPlanAction::Create);
        assert_eq!(
            apply_plan_entries(&plan, false, false, TEST_MCP_SERVER_URL).expect("apply hook"),
            1
        );

        let value: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(root.join(".claude/settings.json")).expect("read settings"),
        )
        .expect("parse settings");
        assert_eq!(
            value["hooks"]["PreToolUse"][0]["hooks"][0],
            super::json_merge::desired_claude_hook_command()
        );

        let plan = build_adopt_plan(
            &config,
            &[AdoptTarget::Hook],
            false,
            false,
            false,
            TEST_MCP_SERVER_URL,
        )
        .expect("build adopt plan again");
        assert_eq!(plan.entries[0].action, super::AdoptPlanAction::Unchanged);
        assert_eq!(
            apply_plan_entries(&plan, false, false, TEST_MCP_SERVER_URL).expect("reapply hook"),
            0
        );
        fs::remove_dir_all(root).expect("remove temp root");
    }

    #[test]
    fn adopt_apply_all_with_explicit_hook_installs_claude_hook() {
        let root = temp_dir("adopt-apply-all-install-hook");
        fs::create_dir_all(&root).expect("create temp root");
        let config = FriggConfig::from_workspace_roots(vec![root.clone()])
            .expect("config should accept workspace root");

        let plan = build_adopt_plan(
            &config,
            &[AdoptTarget::Hook],
            true,
            false,
            false,
            TEST_MCP_SERVER_URL,
        )
        .expect("build adopt plan");
        assert!(
            plan.entries
                .iter()
                .any(|entry| entry.target == AdoptTarget::Hook)
        );

        apply_plan_entries(&plan, false, false, TEST_MCP_SERVER_URL).expect("apply all with hook");
        let value: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(root.join(".claude/settings.json")).expect("read settings"),
        )
        .expect("parse settings");
        assert_eq!(
            value["hooks"]["PreToolUse"][0]["hooks"][0],
            super::json_merge::desired_claude_hook_command()
        );
        fs::remove_dir_all(root).expect("remove temp root");
    }

    #[test]
    fn adopt_apply_preserves_sibling_claude_settings_and_hooks() {
        let root = temp_dir("adopt-apply-preserve-hook");
        fs::create_dir_all(root.join(".claude")).expect("create claude dir");
        fs::write(
            root.join(".claude/settings.json"),
            r#"{"theme":"dark","hooks":{"PreToolUse":[{"matcher":"Write","hooks":[{"type":"command","command":"other"}]}],"PostToolUse":[{"matcher":"Bash","hooks":[{"type":"command","command":"post"}]}]}}"#,
        )
        .expect("write settings");
        let config = FriggConfig::from_workspace_roots(vec![root.clone()])
            .expect("config should accept workspace root");
        let plan = build_adopt_plan(
            &config,
            &[AdoptTarget::Hook],
            false,
            false,
            false,
            TEST_MCP_SERVER_URL,
        )
        .expect("build adopt plan");

        assert_eq!(
            apply_plan_entries(&plan, false, false, TEST_MCP_SERVER_URL).expect("apply hook"),
            1
        );
        let value: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(root.join(".claude/settings.json")).expect("read settings"),
        )
        .expect("parse settings");
        assert_eq!(value["theme"], "dark");
        assert_eq!(
            value["hooks"]["PostToolUse"][0]["hooks"][0]["command"],
            "post"
        );
        assert_eq!(
            value["hooks"]["PreToolUse"][0]["hooks"][0]["command"],
            "other"
        );
        assert_eq!(
            value["hooks"]["PreToolUse"][1]["hooks"][0],
            super::json_merge::desired_claude_hook_command()
        );
        fs::remove_dir_all(root).expect("remove temp root");
    }

    #[test]
    fn adopt_apply_removes_only_frigg_claude_hook_on_uninstall() {
        let root = temp_dir("adopt-apply-remove-hook");
        fs::create_dir_all(root.join(".claude")).expect("create claude dir");
        fs::write(
            root.join(".claude/settings.json"),
            r#"{"hooks":{"PreToolUse":[{"matcher":"Grep|Bash|Read","hooks":[{"type":"command","command":"other"},{"type":"command","command":"frigg hook pretooluse","timeout":5}]},{"matcher":"Write","hooks":[{"type":"command","command":"frigg hook pretooluse","timeout":5},{"type":"command","command":"write-hook"}]}]},"unrelated":true}"#,
        )
        .expect("write settings");
        let config = FriggConfig::from_workspace_roots(vec![root.clone()])
            .expect("config should accept workspace root");
        let plan = build_adopt_plan(
            &config,
            &[AdoptTarget::Hook],
            false,
            true,
            false,
            TEST_MCP_SERVER_URL,
        )
        .expect("build adopt plan");

        assert_eq!(
            apply_plan_entries(&plan, true, false, TEST_MCP_SERVER_URL)
                .expect("apply hook uninstall"),
            1
        );
        let value: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(root.join(".claude/settings.json")).expect("read settings"),
        )
        .expect("parse settings");
        assert_eq!(value["unrelated"], true);
        assert_eq!(
            value["hooks"]["PreToolUse"][0]["hooks"][0]["command"],
            "other"
        );
        assert_eq!(value["hooks"]["PreToolUse"][1]["matcher"], "Write");
        assert_eq!(
            value["hooks"]["PreToolUse"][1]["hooks"][0],
            super::json_merge::desired_claude_hook_command()
        );
        assert_eq!(
            value["hooks"]["PreToolUse"][1]["hooks"][1]["command"],
            "write-hook"
        );
        fs::remove_dir_all(root).expect("remove temp root");
    }

    #[test]
    fn adopt_plan_reports_malformed_claude_settings_without_clobbering() {
        let root = temp_dir("adopt-plan-malformed-hook");
        fs::create_dir_all(root.join(".claude")).expect("create claude dir");
        let settings_path = root.join(".claude/settings.json");
        fs::write(&settings_path, "{not json").expect("write malformed settings");
        let config = FriggConfig::from_workspace_roots(vec![root.clone()])
            .expect("config should accept workspace root");

        let plan = build_adopt_plan(
            &config,
            &[AdoptTarget::Hook],
            false,
            false,
            false,
            TEST_MCP_SERVER_URL,
        )
        .expect("build adopt plan");
        assert_eq!(plan.entries[0].action, super::AdoptPlanAction::Error);

        let err = super::apply_claude_hook_edit(Some("{not json"), false)
            .expect_err("apply should reject malformed JSON");
        assert!(err.to_string().contains("invalid JSON"));
        assert_eq!(
            fs::read_to_string(&settings_path).expect("read settings"),
            "{not json"
        );
        fs::remove_dir_all(root).expect("remove temp root");
    }

    #[cfg(unix)]
    #[test]
    fn adopt_plan_rejects_symlink_target_escape_before_classification_read() {
        let root = temp_dir("adopt-plan-symlink-target-root");
        let outside = temp_dir("adopt-plan-symlink-target-outside");
        fs::create_dir_all(root.join(".cursor")).expect("create cursor dir");
        fs::create_dir_all(&outside).expect("create outside root");
        let outside_mcp = outside.join("mcp.json");
        fs::write(&outside_mcp, "{not json").expect("write outside mcp");
        std::os::unix::fs::symlink(&outside_mcp, root.join(".cursor/mcp.json"))
            .expect("create mcp symlink");
        let config = FriggConfig::from_workspace_roots(vec![root.clone()])
            .expect("config should accept workspace root");

        let plan = build_adopt_plan(
            &config,
            &[AdoptTarget::McpCursor],
            false,
            false,
            false,
            TEST_MCP_SERVER_URL,
        )
        .expect("build adopt plan");

        assert_eq!(plan.entries[0].action, super::AdoptPlanAction::Error);
        let reason = plan.entries[0]
            .reason
            .as_deref()
            .expect("read error reason should be present");
        assert!(
            reason.contains("escapes canonical workspace root boundary"),
            "unexpected reason: {reason}"
        );
        assert!(
            !reason.contains("invalid-json"),
            "classification must not read escaped target contents: {reason}"
        );
        fs::remove_dir_all(root).expect("remove temp root");
        fs::remove_dir_all(outside).expect("remove outside root");
    }

    #[cfg(unix)]
    #[test]
    fn adopt_apply_rejects_symlink_target_escape_before_apply_read() {
        let root = temp_dir("adopt-apply-symlink-target-root");
        let outside = temp_dir("adopt-apply-symlink-target-outside");
        fs::create_dir_all(root.join(".cursor")).expect("create cursor dir");
        fs::create_dir_all(&outside).expect("create outside root");
        let outside_mcp = outside.join("mcp.json");
        fs::write(&outside_mcp, "{not json").expect("write outside mcp");
        std::os::unix::fs::symlink(&outside_mcp, root.join(".cursor/mcp.json"))
            .expect("create mcp symlink");
        let plan = super::AdoptPlan::new(vec![super::AdoptPlanEntry {
            repository_id: "repo-001".to_owned(),
            root: root.clone(),
            target: AdoptTarget::McpCursor,
            path: root.join(AdoptTarget::McpCursor.path()),
            action: super::AdoptPlanAction::Update,
            reason: Some("test-forced-update".to_owned()),
        }]);

        let err = apply_plan_entries(&plan, false, true, TEST_MCP_SERVER_URL)
            .expect_err("symlinked target escape should fail before reading");

        let message = err.to_string();
        assert!(
            message.contains("escapes canonical workspace root boundary"),
            "unexpected error: {message}"
        );
        assert!(
            !message.contains("invalid JSON"),
            "apply must not read escaped target contents: {message}"
        );
        fs::remove_dir_all(root).expect("remove temp root");
        fs::remove_dir_all(outside).expect("remove outside root");
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
        let plan = build_adopt_plan(
            &config,
            &[AdoptTarget::Cursor],
            false,
            false,
            false,
            TEST_MCP_SERVER_URL,
        )
        .expect("build adopt plan");

        let err = apply_plan_entries(&plan, false, false, TEST_MCP_SERVER_URL)
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
        let path = std::env::temp_dir().join(format!("{stem}-{unique}"));
        fs::create_dir_all(path.join(".git")).expect("create fixture git root");
        path
    }
}
