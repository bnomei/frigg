use std::error::Error;
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
    let plan = build_adopt_plan(config, &requested_targets, all, legacy_cursor, uninstall)?;
    let _placeholder_contract = (
        json_merge::DEFAULT_MCP_SERVER_URL,
        json_merge::MCP_SERVER_KEY,
        managed_block::MANAGED_BLOCK_START,
        managed_block::MANAGED_BLOCK_END,
    );
    let status = if plan.is_empty() { "noop" } else { "planned" };

    println!(
        "adopt summary status={} repositories={} targets={} dry_run={} check={} uninstall={} force={} writes=0",
        status,
        config.repositories().len(),
        plan.len(),
        dry_run,
        check,
        uninstall,
        force
    );

    for entry in &plan.entries {
        println!(
            "adopt plan repository_id={} root={} target={:?} path={} action={} writes=0",
            entry.repository_id,
            entry.root.display(),
            entry.target,
            entry.path.display(),
            entry.action.as_str()
        );
    }

    Ok(())
}

fn build_adopt_plan(
    config: &FriggConfig,
    requested_targets: &[AdoptTarget],
    all: bool,
    legacy_cursor: bool,
    uninstall: bool,
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
            entries.push(AdoptPlanEntry {
                repository_id: repo.repository_id.0.clone(),
                root: root.to_path_buf(),
                target,
                path: root.join(target.path()),
                action: if uninstall {
                    AdoptPlanAction::PlanUninstall
                } else {
                    AdoptPlanAction::PlanInstall
                },
            });
        }
    }

    Ok(AdoptPlan::new(entries))
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

        let plan = build_adopt_plan(&config, &[AdoptTarget::McpProject], false, false, false)
            .expect("build adopt plan");

        assert_eq!(plan.len(), 1);
        assert_eq!(plan.entries[0].repository_id, "repo-001");
        assert_eq!(plan.entries[0].path, root.join(".mcp.json"));
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
