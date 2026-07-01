use std::path::Path;

use crate::cli_args::AdoptTarget;

pub(crate) const NON_HOOK_V1_TARGETS: [AdoptTarget; 8] = [
    AdoptTarget::ClaudeMd,
    AdoptTarget::AgentsMd,
    AdoptTarget::GeminiMd,
    AdoptTarget::Copilot,
    AdoptTarget::Cursor,
    AdoptTarget::LegacyCursor,
    AdoptTarget::McpProject,
    AdoptTarget::McpCursor,
];

pub(crate) const DEFAULT_TARGETS: [AdoptTarget; 2] = [AdoptTarget::AgentsMd, AdoptTarget::ClaudeMd];

pub(crate) fn select_targets(
    root: &Path,
    requested_targets: &[AdoptTarget],
    all: bool,
    legacy_cursor: bool,
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

    if legacy_cursor {
        push_unique(&mut targets, AdoptTarget::LegacyCursor);
    }

    targets
}

pub(crate) fn detect_known_project_client_markers(root: &Path) -> Vec<AdoptTarget> {
    let mut targets = Vec::new();
    for target in NON_HOOK_V1_TARGETS {
        if target_marker_exists(root, target) {
            push_unique(&mut targets, target);
        }
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
        fs::write(root.join("GEMINI.md"), "").expect("write gemini marker");
        fs::write(root.join(".cursor/rules/frigg.mdc"), "").expect("write cursor marker");

        let targets = detect_known_project_client_markers(&root);

        assert_eq!(targets, vec![AdoptTarget::GeminiMd, AdoptTarget::Cursor]);
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

    #[test]
    fn adopt_requested_targets_win_over_detection() {
        let root = temp_dir("adopt-requested-targets");
        fs::create_dir_all(&root).expect("create temp root");

        let targets = select_targets(&root, &[AdoptTarget::McpProject], false, true);

        assert_eq!(
            targets,
            vec![AdoptTarget::McpProject, AdoptTarget::LegacyCursor]
        );
        fs::remove_dir_all(root).expect("remove temp root");
    }

    #[test]
    fn adopt_all_does_not_select_hook_target() {
        let root = temp_dir("adopt-all-targets");
        fs::create_dir_all(&root).expect("create temp root");

        let targets = select_targets(&root, &[], true, false);

        assert!(!targets.contains(&AdoptTarget::Hook));
        fs::remove_dir_all(root).expect("remove temp root");
    }

    #[test]
    fn adopt_all_preserves_explicit_hook_target() {
        let root = temp_dir("adopt-all-hook-targets");
        fs::create_dir_all(&root).expect("create temp root");

        let targets = select_targets(&root, &[AdoptTarget::Hook], true, false);

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
