//! Adopt target selection from explicit flags, `--all`, or detected project client markers.
//!
//! Selects adopt destinations from explicit flags, `--all`, or detected agent client markers in the
//! workspace.

use std::path::Path;

use crate::cli_args::AdoptTarget;

pub(crate) const NON_HOOK_V1_TARGETS: [AdoptTarget; 6] = [
    AdoptTarget::ClaudeMd,
    AdoptTarget::AgentsMd,
    AdoptTarget::Copilot,
    AdoptTarget::Cursor,
    AdoptTarget::McpProject,
    AdoptTarget::McpCursor,
];

pub(crate) const DEFAULT_TARGETS: [AdoptTarget; 2] = [AdoptTarget::AgentsMd, AdoptTarget::ClaudeMd];

/// Resolves the adopt target set from `--all`, explicit `--target` flags, or repository markers.
pub(crate) fn select_targets(
    root: &Path,
    requested_targets: &[AdoptTarget],
    all: bool,
) -> Vec<AdoptTarget> {
    let mut targets = if all {
        NON_HOOK_V1_TARGETS.to_vec()
    } else if requested_targets.is_empty() {
        detect_known_project_client_markers(root)
    } else {
        requested_targets.to_vec()
    };

    if all {
        for target in requested_targets {
            push_unique(&mut targets, *target);
        }
    }

    targets
}

/// Infers adopt targets from existing client config files, defaulting to agent docs when none match.
pub(crate) fn detect_known_project_client_markers(root: &Path) -> Vec<AdoptTarget> {
    let mut targets = Vec::new();
    for target in NON_HOOK_V1_TARGETS {
        if target_marker_exists(root, target) {
            push_unique(&mut targets, target);
        }
    }

    // `.claude/` on its own is a weak signal. It shows up for local settings, slash commands, and
    // for the hook `frigg adopt --target hook` installs — so Frigg can create the very directory
    // that changes its own later detection. Before the directory counted, such a repo matched
    // nothing and fell through to DEFAULT_TARGETS, which includes the cross-agent AGENTS.md.
    // Recognizing Claude must not silently stop managing that file.
    let claude_inferred_from_directory_only =
        targets == [AdoptTarget::ClaudeMd] && !root.join(AdoptTarget::ClaudeMd.path()).exists();
    if claude_inferred_from_directory_only {
        push_unique(&mut targets, AdoptTarget::AgentsMd);
    }

    if targets.is_empty() {
        targets.extend(DEFAULT_TARGETS);
    }

    targets
}

fn target_marker_exists(root: &Path, target: AdoptTarget) -> bool {
    match target {
        AdoptTarget::Cursor => {
            root.join(".cursor/rules").is_dir() || root.join(target.path()).exists()
        }
        // A repo can be set up for Claude without a CLAUDE.md: `.claude/` holds settings, skills,
        // and commands. Keying detection on the doc file alone missed those repos entirely, the
        // same way keying Cursor on its rule file alone would miss `.cursor/rules`.
        AdoptTarget::ClaudeMd => root.join(".claude").is_dir() || root.join(target.path()).exists(),
        _ => root.join(target.path()).exists(),
    }
}

fn push_unique(targets: &mut Vec<AdoptTarget>, target: AdoptTarget) {
    if !targets.contains(&target) {
        targets.push(target);
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{detect_known_project_client_markers, select_targets};
    use crate::cli_args::AdoptTarget;

    #[test]
    fn adopt_defaults_to_agents_and_claude_without_markers() {
        let root = temp_dir("adopt-default-targets");
        fs::create_dir_all(&root).expect("create temp root");

        let targets = detect_known_project_client_markers(&root);

        assert_eq!(targets, vec![AdoptTarget::AgentsMd, AdoptTarget::ClaudeMd]);
        fs::remove_dir_all(root).expect("remove temp root");
    }

    #[test]
    fn adopt_detects_existing_project_markers() {
        let root = temp_dir("adopt-detect-targets");
        fs::create_dir_all(root.join(".cursor/rules")).expect("create cursor rules dir");
        fs::create_dir_all(root.join(".github")).expect("create .github dir");
        fs::write(root.join(".github/copilot-instructions.md"), "").expect("write copilot marker");
        fs::write(root.join(".cursor/rules/frigg.mdc"), "").expect("write cursor marker");

        let targets = detect_known_project_client_markers(&root);

        assert_eq!(targets, vec![AdoptTarget::Copilot, AdoptTarget::Cursor]);
        fs::remove_dir_all(root).expect("remove temp root");
    }

    #[test]
    fn adopt_detects_cursor_project_marker_without_frigg_rule() {
        let root = temp_dir("adopt-detect-cursor-project-marker");
        fs::create_dir_all(root.join(".cursor/rules")).expect("create cursor rules dir");

        let targets = detect_known_project_client_markers(&root);

        assert_eq!(targets, vec![AdoptTarget::Cursor]);
        fs::remove_dir_all(root).expect("remove temp root");
    }

    /// A repo can be Claude-configured through `.claude/` alone, with no CLAUDE.md.
    #[test]
    fn adopt_detects_claude_directory_without_claude_md() {
        let root = temp_dir("adopt-detect-claude-dir");
        fs::create_dir_all(root.join(".claude")).expect("create .claude dir");

        let targets = detect_known_project_client_markers(&root);

        assert!(
            targets.contains(&AdoptTarget::ClaudeMd),
            "detection must not depend on the doc file existing"
        );
        assert!(
            targets.contains(&AdoptTarget::AgentsMd),
            "the directory alone must not drop the cross-agent doc the default would have written"
        );
        fs::remove_dir_all(root).expect("remove temp root");
    }

    /// Frigg's own `--target hook` creates `.claude/`. That must not change what a later bare
    /// `frigg adopt` manages in the same repo.
    #[test]
    fn frigg_created_claude_dir_does_not_narrow_the_default_target_set() {
        let root = temp_dir("adopt-claude-dir-feedback");
        fs::create_dir_all(&root).expect("create temp root");
        let before = detect_known_project_client_markers(&root);

        // Simulate `frigg adopt --target hook`.
        fs::create_dir_all(root.join(".claude")).expect("create .claude dir");
        fs::write(root.join(".claude/settings.json"), "{}").expect("write settings");
        let after = detect_known_project_client_markers(&root);

        for target in &before {
            assert!(
                after.contains(target),
                "{target:?} was managed before the hook install and must still be after"
            );
        }
        fs::remove_dir_all(root).expect("remove temp root");
    }

    /// A real CLAUDE.md is a deliberate choice, so it does not pull in AGENTS.md.
    #[test]
    fn claude_md_file_alone_stays_a_claude_only_target_set() {
        let root = temp_dir("adopt-claude-md-only");
        fs::create_dir_all(&root).expect("create temp root");
        fs::write(root.join("CLAUDE.md"), "").expect("write claude md");

        let targets = detect_known_project_client_markers(&root);

        assert_eq!(targets, vec![AdoptTarget::ClaudeMd]);
        fs::remove_dir_all(root).expect("remove temp root");
    }

    #[test]
    fn adopt_requested_targets_win_over_detection() {
        let root = temp_dir("adopt-requested-targets");
        fs::create_dir_all(&root).expect("create temp root");

        let targets = select_targets(&root, &[AdoptTarget::McpProject], false);

        assert_eq!(targets, vec![AdoptTarget::McpProject]);
        fs::remove_dir_all(root).expect("remove temp root");
    }

    #[test]
    fn adopt_all_does_not_select_hook_target() {
        let root = temp_dir("adopt-all-targets");
        fs::create_dir_all(&root).expect("create temp root");

        let targets = select_targets(&root, &[], true);

        assert!(!targets.contains(&AdoptTarget::Hook));
        fs::remove_dir_all(root).expect("remove temp root");
    }

    #[test]
    fn adopt_all_preserves_explicit_hook_target() {
        let root = temp_dir("adopt-all-hook-targets");
        fs::create_dir_all(&root).expect("create temp root");

        let targets = select_targets(&root, &[AdoptTarget::Hook], true);

        assert!(targets.contains(&AdoptTarget::Hook));
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
