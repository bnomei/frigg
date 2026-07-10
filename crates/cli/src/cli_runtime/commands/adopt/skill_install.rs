//! Best-effort install of the production `frigg-first-code-search` skill into host skill dirs.
//!
//! Never creates a missing parent `…/skills` directory. Only copies into an existing skills root
//! (or removes the skill tree on uninstall). Destination layout is researched host defaults.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::cli_args::SkillProvider;

/// Skill directory name under each host skills root.
pub(crate) const SKILL_DIR_NAME: &str = "frigg-first-code-search";

/// Relative path of the canonical skill tree inside a Frigg workspace / monorepo.
pub(crate) const WORKSPACE_SKILL_REL: &str = "skills/frigg-first-code-search";

/// Env override for the skill source directory (must contain `SKILL.md`).
pub(crate) const SKILL_SOURCE_ENV: &str = "FRIGG_SKILL_SOURCE";

/// Planned skill install outcome for one provider under one workspace root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SkillInstallPlan {
    pub(crate) provider: SkillProvider,
    pub(crate) skills_parent: Option<PathBuf>,
    pub(crate) dest: Option<PathBuf>,
    pub(crate) action: SkillInstallAction,
    pub(crate) reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SkillInstallAction {
    Create,
    Update,
    Unchanged,
    Remove,
    Skipped,
    Error,
}

impl SkillInstallAction {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::Update => "update",
            Self::Unchanged => "unchanged",
            Self::Remove => "remove",
            Self::Skipped => "skipped",
            Self::Error => "error",
        }
    }

    pub(crate) fn is_pending_change(self) -> bool {
        matches!(
            self,
            Self::Create | Self::Update | Self::Remove | Self::Error
        )
    }
}

impl SkillProvider {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Cursor => "cursor",
            Self::Copilot => "copilot",
        }
    }
}

/// Resolve source skill directory: `FRIGG_SKILL_SOURCE`, else workspace-relative tree.
pub(crate) fn resolve_skill_source(workspace_root: &Path) -> Result<PathBuf, String> {
    if let Some(from_env) = std::env::var_os(SKILL_SOURCE_ENV) {
        let path = PathBuf::from(from_env);
        return validate_skill_source(&path);
    }

    let candidate = workspace_root.join(WORKSPACE_SKILL_REL);
    validate_skill_source(&candidate)
}

fn validate_skill_source(path: &Path) -> Result<PathBuf, String> {
    let skill_md = if path.is_file()
        && path
            .file_name()
            .is_some_and(|name| name == "SKILL.md" || name == "skill.md")
    {
        path.to_path_buf()
    } else {
        path.join("SKILL.md")
    };

    if !skill_md.is_file() {
        return Err(format!(
            "skill source missing SKILL.md at {} (set {SKILL_SOURCE_ENV} or add {WORKSPACE_SKILL_REL})",
            skill_md.display()
        ));
    }

    let dir = skill_md
        .parent()
        .ok_or_else(|| "skill source path has no parent directory".to_owned())?
        .to_path_buf();
    Ok(dir)
}

/// Candidate parent skills directories for a provider (first existing dir wins).
///
/// Best-effort researched defaults (macOS/`~` style; same relative layout on Linux):
/// - Claude: `~/.claude/skills`, then project `.claude/skills`
/// - Codex: `~/.codex/skills`
/// - Cursor: project `.cursor/skills`, then `~/.cursor/skills`
/// - Copilot: project `.github/skills` (CI), then `~/.copilot/skills`
pub(crate) fn skill_parent_candidates(
    provider: SkillProvider,
    workspace_root: &Path,
    home: Option<&Path>,
) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    match provider {
        SkillProvider::Claude => {
            if let Some(home) = home {
                candidates.push(home.join(".claude/skills"));
            }
            candidates.push(workspace_root.join(".claude/skills"));
        }
        SkillProvider::Codex => {
            if let Some(home) = home {
                candidates.push(home.join(".codex/skills"));
            }
        }
        SkillProvider::Cursor => {
            candidates.push(workspace_root.join(".cursor/skills"));
            if let Some(home) = home {
                candidates.push(home.join(".cursor/skills"));
            }
        }
        SkillProvider::Copilot => {
            candidates.push(workspace_root.join(".github/skills"));
            if let Some(home) = home {
                candidates.push(home.join(".copilot/skills"));
            }
        }
    }
    candidates
}

/// Pick the first candidate parent skills directory that already exists as a directory.
pub(crate) fn resolve_existing_skills_parent(
    provider: SkillProvider,
    workspace_root: &Path,
    home: Option<&Path>,
) -> Option<PathBuf> {
    skill_parent_candidates(provider, workspace_root, home)
        .into_iter()
        .find(|path| path.is_dir())
}

/// Plan skill install/uninstall for one provider without writing.
pub(crate) fn plan_skill_install(
    provider: SkillProvider,
    workspace_root: &Path,
    home: Option<&Path>,
    source: Option<&Path>,
    uninstall: bool,
) -> SkillInstallPlan {
    let Some(skills_parent) = resolve_existing_skills_parent(provider, workspace_root, home)
    else {
        let tried = skill_parent_candidates(provider, workspace_root, home)
            .into_iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join(", ");
        return SkillInstallPlan {
            provider,
            skills_parent: None,
            dest: None,
            action: SkillInstallAction::Skipped,
            reason: format!(
                "skills-parent-missing: never create parent skills dir (tried: {tried})"
            ),
        };
    };

    let dest = skills_parent.join(SKILL_DIR_NAME);

    if uninstall {
        if dest.is_dir() {
            return SkillInstallPlan {
                provider,
                skills_parent: Some(skills_parent),
                dest: Some(dest),
                action: SkillInstallAction::Remove,
                reason: "skill-tree-present".to_owned(),
            };
        }
        return SkillInstallPlan {
            provider,
            skills_parent: Some(skills_parent),
            dest: Some(dest),
            action: SkillInstallAction::Unchanged,
            reason: "skill-tree-absent".to_owned(),
        };
    }

    let Some(source) = source else {
        return SkillInstallPlan {
            provider,
            skills_parent: Some(skills_parent),
            dest: Some(dest),
            action: SkillInstallAction::Skipped,
            reason: format!(
                "skill-source-missing: need {WORKSPACE_SKILL_REL} or {SKILL_SOURCE_ENV}"
            ),
        };
    };

    if !source.join("SKILL.md").is_file() {
        return SkillInstallPlan {
            provider,
            skills_parent: Some(skills_parent),
            dest: Some(dest),
            action: SkillInstallAction::Skipped,
            reason: format!("skill-source-invalid:{}", source.display()),
        };
    }

    if dest_matches_source(source, &dest) {
        return SkillInstallPlan {
            provider,
            skills_parent: Some(skills_parent),
            dest: Some(dest),
            action: SkillInstallAction::Unchanged,
            reason: "skill-tree-current".to_owned(),
        };
    }

    let action = if dest.is_dir() {
        SkillInstallAction::Update
    } else {
        SkillInstallAction::Create
    };

    SkillInstallPlan {
        provider,
        skills_parent: Some(skills_parent),
        dest: Some(dest),
        action,
        reason: format!("source:{}", source.display()),
    }
}

/// Apply a planned skill install (copy tree or remove). Does not create parent skills dirs.
pub(crate) fn apply_skill_install(
    plan: &SkillInstallPlan,
    source: Option<&Path>,
) -> io::Result<()> {
    match plan.action {
        SkillInstallAction::Skipped | SkillInstallAction::Unchanged => Ok(()),
        SkillInstallAction::Error => Err(io::Error::other(plan.reason.clone())),
        SkillInstallAction::Remove => {
            let dest = plan
                .dest
                .as_ref()
                .ok_or_else(|| io::Error::other("skill remove missing dest"))?;
            if dest.is_dir() {
                fs::remove_dir_all(dest)?;
            }
            Ok(())
        }
        SkillInstallAction::Create | SkillInstallAction::Update => {
            let dest = plan
                .dest
                .as_ref()
                .ok_or_else(|| io::Error::other("skill install missing dest"))?;
            let source = source.ok_or_else(|| io::Error::other("skill install missing source"))?;
            // Parent skills dir must already exist (plan-time check); only create the skill leaf.
            if let Some(parent) = plan.skills_parent.as_ref() {
                if !parent.is_dir() {
                    return Err(io::Error::new(
                        io::ErrorKind::NotFound,
                        format!(
                            "refusing to create missing skills parent {}",
                            parent.display()
                        ),
                    ));
                }
            }
            copy_skill_tree(source, dest)
        }
    }
}

fn dest_matches_source(source: &Path, dest: &Path) -> bool {
    let Ok(source_md) = fs::read_to_string(source.join("SKILL.md")) else {
        return false;
    };
    let Ok(dest_md) = fs::read_to_string(dest.join("SKILL.md")) else {
        return false;
    };
    // Cheap equality: SKILL.md is the versioned surface; full tree copy still on update.
    source_md == dest_md && dest.is_dir()
}

fn copy_skill_tree(source: &Path, dest: &Path) -> io::Result<()> {
    if dest.exists() {
        fs::remove_dir_all(dest)?;
    }
    fs::create_dir_all(dest)?;
    copy_dir_recursive(source, dest)
}

fn copy_dir_recursive(source: &Path, dest: &Path) -> io::Result<()> {
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let from = entry.path();
        let to = dest.join(entry.file_name());
        if file_type.is_dir() {
            fs::create_dir_all(&to)?;
            copy_dir_recursive(&from, &to)?;
        } else if file_type.is_file() {
            fs::copy(&from, &to)?;
        }
        // Skip symlinks and special files for safety.
    }
    Ok(())
}

/// Resolve `$HOME` for skill path candidates (macOS/Linux).
pub(crate) fn resolve_home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn skips_when_skills_parent_missing() {
        let root = temp_dir("skill-skip-parent");
        fs::create_dir_all(&root).expect("root");
        let home = temp_dir("skill-skip-home");
        fs::create_dir_all(&home).expect("home");

        let plan = plan_skill_install(
            SkillProvider::Claude,
            &root,
            Some(&home),
            None,
            false,
        );
        assert_eq!(plan.action, SkillInstallAction::Skipped);
        assert!(plan.reason.contains("skills-parent-missing"));
        assert!(!home.join(".claude/skills").exists());

        fs::remove_dir_all(root).ok();
        fs::remove_dir_all(home).ok();
    }

    #[test]
    fn installs_into_existing_claude_skills_parent() {
        let root = temp_dir("skill-install-root");
        let home = temp_dir("skill-install-home");
        let source = temp_dir("skill-install-source");
        fs::create_dir_all(&root).expect("root");
        fs::create_dir_all(home.join(".claude/skills")).expect("skills parent");
        fs::create_dir_all(source.join("references")).expect("source refs");
        fs::write(source.join("SKILL.md"), "version=test-skill\n").expect("skill md");
        fs::write(source.join("references/a.md"), "ref\n").expect("ref");

        let plan = plan_skill_install(
            SkillProvider::Claude,
            &root,
            Some(&home),
            Some(&source),
            false,
        );
        assert_eq!(plan.action, SkillInstallAction::Create);
        apply_skill_install(&plan, Some(&source)).expect("apply");

        let dest = home.join(".claude/skills").join(SKILL_DIR_NAME);
        assert!(dest.join("SKILL.md").is_file());
        assert!(dest.join("references/a.md").is_file());

        // Unchanged on second plan with same content.
        let plan2 = plan_skill_install(
            SkillProvider::Claude,
            &root,
            Some(&home),
            Some(&source),
            false,
        );
        assert_eq!(plan2.action, SkillInstallAction::Unchanged);

        fs::remove_dir_all(root).ok();
        fs::remove_dir_all(home).ok();
        fs::remove_dir_all(source).ok();
    }

    #[test]
    fn copilot_prefers_project_github_skills_for_ci() {
        let root = temp_dir("skill-copilot-root");
        let home = temp_dir("skill-copilot-home");
        fs::create_dir_all(root.join(".github/skills")).expect("project skills");
        fs::create_dir_all(home.join(".copilot/skills")).expect("personal skills");

        let parent = resolve_existing_skills_parent(SkillProvider::Copilot, &root, Some(&home));
        assert_eq!(parent, Some(root.join(".github/skills")));

        fs::remove_dir_all(root).ok();
        fs::remove_dir_all(home).ok();
    }

    #[test]
    fn never_creates_skills_parent_on_apply_error_path() {
        let root = temp_dir("skill-no-mkdir-root");
        fs::create_dir_all(&root).expect("root");
        let home = temp_dir("skill-no-mkdir-home");
        fs::create_dir_all(&home).expect("home");
        // No .claude/skills created.
        let plan = plan_skill_install(SkillProvider::Claude, &root, Some(&home), None, false);
        assert_eq!(plan.action, SkillInstallAction::Skipped);
        apply_skill_install(&plan, None).expect("skip is ok");
        assert!(!home.join(".claude").exists());

        fs::remove_dir_all(root).ok();
        fs::remove_dir_all(home).ok();
    }

    fn temp_dir(stem: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        std::env::temp_dir().join(format!("frigg-{stem}-{unique}"))
    }
}
