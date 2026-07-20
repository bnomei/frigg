//! Best-effort install of the production `frigg-first-code-search` skill into host skill dirs.
//!
//! Copies into an existing skills root, or removes the skill tree on uninstall. Destination
//! layout is researched host defaults. The parent `…/skills` directory is never invented, with one
//! exception: Claude's documented `~/.claude/skills` leaf, and only when `~/.claude` already exists.

use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

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
    /// Source skill tree used for this plan (install only); apply uses this path.
    pub(crate) source: Option<PathBuf>,
    /// The one directory this plan authorizes creating, if any.
    ///
    /// Holds the path rather than a permission bit so apply can confirm it is creating exactly
    /// what planning approved, and never re-derives the decision from ambient environment state.
    pub(crate) parent_to_create: Option<PathBuf>,
    pub(crate) action: SkillInstallAction,
    pub(crate) reason: String,
}

/// Planned or applied skill-tree install action for host skill directories.
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
            Self::Amp => "amp",
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
    let skill_md = if path.is_file() && path.file_name().is_some_and(|name| name == "SKILL.md") {
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
/// - Amp: `~/.config/agents/skills`, then `~/.config/amp/skills`, then project `.agents/skills`
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
        SkillProvider::Amp => {
            if let Some(home) = home {
                candidates.push(home.join(".config/agents/skills"));
                candidates.push(home.join(".config/amp/skills"));
            }
            candidates.push(workspace_root.join(".agents/skills"));
        }
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
///
/// An existing parent always wins over one Frigg could create, even when the creatable one ranks
/// higher in `skill_parent_candidates`. A repo with `.claude/skills` checked in therefore keeps
/// receiving the install, rather than having it move to a freshly created `~/.claude/skills`.
pub(crate) fn resolve_existing_skills_parent(
    provider: SkillProvider,
    workspace_root: &Path,
    home: Option<&Path>,
) -> Option<PathBuf> {
    skill_parent_candidates(provider, workspace_root, home)
        .into_iter()
        .find(|path| path.is_dir())
}

/// A skills parent Frigg may create when none exists yet.
///
/// Refusing to create the parent is the right default for a host whose layout Frigg is guessing:
/// inventing `…/skills` somewhere wrong leaves litter the user never asked for. Claude is not a
/// guess. `~/.claude/skills` is the documented personal skills directory, so on a machine where
/// the user has Claude but has never written a skill, skipping produces no skill and an install
/// that quietly did nothing.
///
/// The gate is `~/.claude` already existing. That directory is Claude's own, so its presence means
/// Claude is set up here; its absence means Frigg would be creating a config tree for a tool that
/// is not installed. Only the `skills` leaf is ever created, never `~/.claude` itself.
pub(crate) fn creatable_skills_parent(
    provider: SkillProvider,
    home: Option<&Path>,
) -> Option<PathBuf> {
    if provider != SkillProvider::Claude {
        return None;
    }
    let home = home?;
    let claude_home = home.join(".claude");
    claude_home
        .is_dir()
        .then(|| claude_home.join("skills"))
        .filter(|skills| !skills.exists())
}

/// Plan skill install/uninstall for one provider without writing.
pub(crate) fn plan_skill_install(
    provider: SkillProvider,
    workspace_root: &Path,
    home: Option<&Path>,
    source: Option<&Path>,
    uninstall: bool,
) -> SkillInstallPlan {
    let existing_parent = resolve_existing_skills_parent(provider, workspace_root, home);
    // Uninstall never creates anything: with no parent there is nothing installed to remove.
    let creatable_parent = if uninstall {
        None
    } else {
        creatable_skills_parent(provider, home)
    };

    // Record which source won: apply confirms against this exact path before any mkdir.
    let parent_to_create = existing_parent
        .is_none()
        .then(|| creatable_parent.clone())
        .flatten();
    let Some(skills_parent) = existing_parent.or(creatable_parent) else {
        let tried = skill_parent_candidates(provider, workspace_root, home)
            .into_iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join(", ");
        return SkillInstallPlan {
            provider,
            skills_parent: None,
            dest: None,
            source: None,
            parent_to_create: None,
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
                source: None,
                parent_to_create: None,
                action: SkillInstallAction::Remove,
                reason: "skill-tree-present".to_owned(),
            };
        }
        return SkillInstallPlan {
            provider,
            skills_parent: Some(skills_parent),
            dest: Some(dest),
            source: None,
            parent_to_create: None,
            action: SkillInstallAction::Unchanged,
            reason: "skill-tree-absent".to_owned(),
        };
    }

    let Some(source) = source else {
        return SkillInstallPlan {
            provider,
            skills_parent: Some(skills_parent),
            dest: Some(dest),
            source: None,
            parent_to_create: None,
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
            source: Some(source.to_path_buf()),
            parent_to_create: None,
            action: SkillInstallAction::Skipped,
            reason: format!("skill-source-invalid:{}", source.display()),
        };
    }

    if dest.exists() && !dest.is_dir() {
        return SkillInstallPlan {
            provider,
            skills_parent: Some(skills_parent),
            dest: Some(dest),
            source: Some(source.to_path_buf()),
            parent_to_create: None,
            action: SkillInstallAction::Error,
            reason: "dest-not-directory".to_owned(),
        };
    }

    if trees_match(source, &dest, provider) {
        return SkillInstallPlan {
            provider,
            skills_parent: Some(skills_parent),
            dest: Some(dest),
            source: Some(source.to_path_buf()),
            parent_to_create: None,
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
        source: Some(source.to_path_buf()),
        parent_to_create: parent_to_create.clone(),
        action,
        reason: match &parent_to_create {
            // Creating a directory under the user's home is the one exception to this module's
            // never-invent-a-parent rule, so it has to be visible in --dry-run and in the log.
            Some(path) => format!(
                "source:{} creating-skills-parent:{}",
                source.display(),
                path.display()
            ),
            None => format!("source:{}", source.display()),
        },
    }
}

/// Apply a planned skill install (copy tree or remove).
///
/// Creates the parent skills directory only when the plan set `create_parent`.
pub(crate) fn apply_skill_install(plan: &SkillInstallPlan) -> io::Result<()> {
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
            } else if dest.exists() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("skill remove dest is not a directory: {}", dest.display()),
                ));
            }
            Ok(())
        }
        SkillInstallAction::Create | SkillInstallAction::Update => {
            let dest = plan
                .dest
                .as_ref()
                .ok_or_else(|| io::Error::other("skill install missing dest"))?;
            let source = plan
                .source
                .as_ref()
                .ok_or_else(|| io::Error::other("skill install missing source"))?;
            let parent = plan
                .skills_parent
                .as_ref()
                .ok_or_else(|| io::Error::other("skill install missing skills parent"))?;
            if !parent.is_dir() {
                // Only the parent the plan itself judged creatable, and only its leaf directory:
                // `create_dir` rather than `create_dir_all` so a missing grandparent still fails
                // instead of materializing a config tree for a host that is not installed.
                if plan.parent_to_create.as_deref() == Some(parent.as_path()) {
                    fs::create_dir(parent).map_err(|err| {
                        // `exists()` follows symlinks, so a dangling link reads as absent during
                        // planning and then collides here. Say that rather than the bare EEXIST.
                        let detail = if fs::symlink_metadata(parent).is_ok() {
                            "path already exists as a symlink or non-directory; remove or repoint it"
                        } else {
                            "could not create the directory"
                        };
                        io::Error::new(
                            err.kind(),
                            format!(
                                "failed to create skills parent {}: {detail} ({err})",
                                parent.display()
                            ),
                        )
                    })?;
                } else {
                    return Err(io::Error::new(
                        io::ErrorKind::NotFound,
                        format!(
                            "refusing to create missing skills parent {}",
                            parent.display()
                        ),
                    ));
                }
            }
            if dest.exists() && !dest.is_dir() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!(
                        "skill dest exists and is not a directory: {}",
                        dest.display()
                    ),
                ));
            }
            copy_skill_tree_atomic(source, dest, parent, plan.provider)
        }
    }
}

/// Bundled assets that belong to a single host, keyed by `/`-separated relative path.
///
/// `mcp.json` is Amp's config shape: a flat `{"frigg": {...}}` map with `includeTools`. Claude
/// reads neither that filename nor that shape (it wants `.mcp.json` with an `mcpServers` wrapper,
/// and does not implement `includeTools` at all), so shipping it to other hosts installs a file
/// they will never read. Assets absent from this table go to every provider.
/// The Claude entries turn the copied tree into a skills-directory plugin: Claude Code loads any
/// folder under a skills dir that holds `.claude-plugin/plugin.json` as a plugin, picking up the
/// sibling `.mcp.json` and `hooks/hooks.json` with it. One `--skill-provider claude` therefore
/// wires skill, MCP server, and PreToolUse hook together. Other hosts ignore all three.
const PROVIDER_SCOPED_ASSETS: &[(&str, SkillProvider)] = &[
    ("mcp.json", SkillProvider::Amp),
    (".claude-plugin/plugin.json", SkillProvider::Claude),
    (".mcp.json", SkillProvider::Claude),
    ("hooks/hooks.json", SkillProvider::Claude),
];

/// Assets withheld from one host while still shipping to every other host.
///
/// `agents/openai.yaml` is the OpenAI prompt variant. Claude reads it as nothing, but `agents/` is
/// a default Claude plugin component directory, and a plugin that has both a default folder and a
/// manifest key overriding it makes Claude Code report an ignored folder in `claude plugin list`
/// and the `/plugin` view. Withholding the file leaves the Claude install with no `agents/` at
/// all, so no override is needed and no warning appears. Hosts that already receive it keep it.
const PROVIDER_EXCLUDED_ASSETS: &[(&str, SkillProvider)] =
    &[("agents/openai.yaml", SkillProvider::Claude)];

/// The provider a bundled skill asset belongs to, when it is not meant for every host.
fn asset_owner(rel: &Path) -> Option<SkillProvider> {
    let key = relative_asset_key(rel)?;
    PROVIDER_SCOPED_ASSETS
        .iter()
        .find(|(path, _)| *path == key)
        .map(|(_, owner)| *owner)
}

/// True when this asset is explicitly withheld from `provider`.
fn asset_is_excluded_for(provider: SkillProvider, rel: &Path) -> bool {
    let Some(key) = relative_asset_key(rel) else {
        return false;
    };
    PROVIDER_EXCLUDED_ASSETS
        .iter()
        .any(|(path, excluded)| *path == key && *excluded == provider)
}

/// Normalize a relative path into a `/`-separated lookup key.
///
/// `collect_relative_files` builds paths from `read_dir`, so on Windows a nested asset arrives as
/// `.claude-plugin\plugin.json`. Comparing that against a `/`-separated table entry would never
/// match, and the miss would fail open: a host-specific file would install for every provider.
fn relative_asset_key(rel: &Path) -> Option<String> {
    let mut parts = Vec::new();
    for component in rel.components() {
        match component {
            Component::Normal(part) => parts.push(part.to_str()?),
            // A bundled asset path is always a plain relative chain of names.
            _ => return None,
        }
    }
    Some(parts.join("/"))
}

/// True when `provider` should receive the bundled asset at repository-relative `rel`.
fn provider_includes_asset(provider: SkillProvider, rel: &Path) -> bool {
    if asset_is_excluded_for(provider, rel) {
        return false;
    }
    asset_owner(rel).is_none_or(|owner| owner == provider)
}

/// Relative files from the bundled tree that `provider` should receive.
fn provider_source_files(source: &Path, provider: SkillProvider) -> io::Result<Vec<PathBuf>> {
    Ok(collect_relative_files(source)?
        .into_iter()
        .filter(|rel| provider_includes_asset(provider, rel))
        .collect())
}

/// True when dest exists and every relative file this provider should receive matches dest bytes
/// (and dest has no extra files). Symlinks are ignored on both sides.
fn trees_match(source: &Path, dest: &Path, provider: SkillProvider) -> bool {
    if !dest.is_dir() {
        return false;
    }
    let Ok(source_files) = provider_source_files(source, provider) else {
        return false;
    };
    let Ok(dest_files) = collect_relative_files(dest) else {
        return false;
    };
    if source_files.len() != dest_files.len() {
        return false;
    }
    for rel in &source_files {
        if !dest_files.contains(rel) {
            return false;
        }
        let Ok(src_bytes) = fs::read(source.join(rel)) else {
            return false;
        };
        let Ok(dst_bytes) = fs::read(dest.join(rel)) else {
            return false;
        };
        if src_bytes != dst_bytes {
            return false;
        }
    }
    true
}

fn collect_relative_files(root: &Path) -> io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    collect_relative_files_rec(root, root, &mut files)?;
    files.sort();
    Ok(files)
}

fn collect_relative_files_rec(
    root: &Path,
    current: &Path,
    files: &mut Vec<PathBuf>,
) -> io::Result<()> {
    for entry in fs::read_dir(current)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let path = entry.path();
        if file_type.is_dir() {
            collect_relative_files_rec(root, &path, files)?;
        } else if file_type.is_file() {
            let rel = path
                .strip_prefix(root)
                .map_err(|err| io::Error::other(format!("strip prefix: {err}")))?
                .to_path_buf();
            files.push(rel);
        }
    }
    Ok(())
}

/// Stage the assets this provider should receive under the existing skills parent, then
/// rename into place. Leaves the previous dest intact if staging fails.
///
/// Promotion swaps the whole directory, so an asset a previous Frigg installed but this provider
/// no longer owns is dropped rather than merged forward.
fn copy_skill_tree_atomic(
    source: &Path,
    dest: &Path,
    skills_parent: &Path,
    provider: SkillProvider,
) -> io::Result<()> {
    let staging_name = format!(".{SKILL_DIR_NAME}.staging-{}", std::process::id());
    let staging = skills_parent.join(&staging_name);
    if staging.exists() {
        if staging.is_dir() {
            fs::remove_dir_all(&staging)?;
        } else {
            fs::remove_file(&staging)?;
        }
    }

    fs::create_dir(&staging).map_err(|err| {
        io::Error::new(
            err.kind(),
            format!(
                "failed to create skill staging dir under existing skills parent {}: {err}",
                skills_parent.display()
            ),
        )
    })?;

    if let Err(err) = copy_provider_assets(source, &staging, provider) {
        let _ = fs::remove_dir_all(&staging);
        return Err(err);
    }

    let backup_name = format!(".{SKILL_DIR_NAME}.backup-{}", std::process::id());
    let backup = skills_parent.join(&backup_name);
    if dest.exists() {
        if backup.exists() {
            if backup.is_dir() {
                fs::remove_dir_all(&backup)?;
            } else {
                fs::remove_file(&backup)?;
            }
        }
        fs::rename(dest, &backup).map_err(|err| {
            let _ = fs::remove_dir_all(&staging);
            io::Error::new(
                err.kind(),
                format!("failed to move existing skill tree aside: {err}"),
            )
        })?;
    }

    if let Err(err) = fs::rename(&staging, dest) {
        if backup.exists() {
            let _ = fs::rename(&backup, dest);
        }
        let _ = fs::remove_dir_all(&staging);
        return Err(io::Error::new(
            err.kind(),
            format!("failed to promote staged skill tree: {err}"),
        ));
    }

    if backup.exists() {
        let _ = fs::remove_dir_all(&backup);
    }
    Ok(())
}

/// Copy the assets `provider` should receive, recreating intermediate directories as needed.
///
/// Files only: an empty source directory is not replicated. `trees_match` likewise compares only
/// files, so the two stay in agreement and the install still converges.
fn copy_provider_assets(source: &Path, dest: &Path, provider: SkillProvider) -> io::Result<()> {
    for rel in provider_source_files(source, provider)? {
        let to = dest.join(&rel);
        if let Some(parent) = to.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(source.join(&rel), &to)?;
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
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    /// Amp is the one host that reads `mcp.json`, so it must still receive it byte-for-byte, and
    /// the install must settle on Unchanged rather than rewriting every run.
    #[test]
    fn amp_still_receives_bundled_mcp_json_and_converges() {
        let root = temp_dir("skill-amp-root");
        let home = temp_dir("skill-amp-home");
        let source = temp_dir("skill-amp-source");
        fs::create_dir_all(&root).expect("root");
        fs::create_dir_all(home.join(".config/agents/skills")).expect("skills parent");
        fs::create_dir_all(&source).expect("source");
        fs::write(source.join("SKILL.md"), "version=test-skill\n").expect("skill md");
        fs::write(source.join("mcp.json"), "{\"frigg\":{}}\n").expect("skill mcp");

        let plan = plan_skill_install(SkillProvider::Amp, &root, Some(&home), Some(&source), false);
        assert_eq!(
            plan.action,
            SkillInstallAction::Create,
            "reason={} parent={:?}",
            plan.reason,
            plan.skills_parent
        );
        apply_skill_install(&plan).expect("apply");

        let dest = home.join(".config/agents/skills").join(SKILL_DIR_NAME);
        assert_eq!(
            fs::read_to_string(dest.join("mcp.json")).expect("read skill mcp"),
            "{\"frigg\":{}}\n"
        );

        let plan2 =
            plan_skill_install(SkillProvider::Amp, &root, Some(&home), Some(&source), false);
        assert_eq!(
            plan2.action,
            SkillInstallAction::Unchanged,
            "amp install must converge"
        );

        fs::remove_dir_all(root).ok();
        fs::remove_dir_all(home).ok();
        fs::remove_dir_all(source).ok();
    }

    /// The upgrade path: an older Frigg copied the whole tree, so an existing install carries an
    /// asset this provider no longer owns. Adopting again must drop it and then settle.
    #[test]
    fn install_drops_asset_left_by_a_previous_unfiltered_install() {
        let root = temp_dir("skill-stale-root");
        let home = temp_dir("skill-stale-home");
        let source = temp_dir("skill-stale-source");
        fs::create_dir_all(&root).expect("root");
        fs::create_dir_all(home.join(".claude/skills")).expect("skills parent");
        fs::create_dir_all(&source).expect("source");
        fs::write(source.join("SKILL.md"), "version=test-skill\n").expect("skill md");
        fs::write(source.join("mcp.json"), "{\"frigg\":{}}\n").expect("skill mcp");

        // What an older, unfiltered Frigg would have left behind.
        let dest = home.join(".claude/skills").join(SKILL_DIR_NAME);
        fs::create_dir_all(&dest).expect("dest");
        fs::write(dest.join("SKILL.md"), "version=test-skill\n").expect("stale skill md");
        fs::write(dest.join("mcp.json"), "{\"frigg\":{}}\n").expect("stale mcp");

        let plan = plan_skill_install(
            SkillProvider::Claude,
            &root,
            Some(&home),
            Some(&source),
            false,
        );
        assert_eq!(
            plan.action,
            SkillInstallAction::Update,
            "a stale unowned asset is pending work"
        );
        apply_skill_install(&plan).expect("apply");

        assert!(
            !dest.join("mcp.json").exists(),
            "the unowned asset must be gone, not merged forward"
        );
        assert!(dest.join("SKILL.md").is_file());

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

    /// A typo in a scoped-asset key fails open to "shared with everyone", which is silent. Assert
    /// every entry names a file that actually exists in the bundled tree.
    #[test]
    fn provider_scoped_asset_table_matches_the_bundled_tree() {
        let bundled = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(WORKSPACE_SKILL_REL);
        if !bundled.is_dir() {
            return;
        }

        for (rel, _provider) in PROVIDER_SCOPED_ASSETS
            .iter()
            .chain(PROVIDER_EXCLUDED_ASSETS)
        {
            assert!(
                bundled.join(rel).is_file(),
                "scoped asset {rel} is not in the bundled tree; the key would silently fail open \
                 and install for every provider"
            );
        }
    }

    /// Guards the manifest fields that are easy to get silently wrong: a version pin that drifts
    /// from the crate, and the skills path that older Claude Code needs to load the root SKILL.md.
    #[test]
    fn bundled_claude_plugin_manifest_is_well_formed() {
        let bundled = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(WORKSPACE_SKILL_REL);
        let Ok(contents) = fs::read_to_string(bundled.join(".claude-plugin/plugin.json")) else {
            return;
        };
        let value: serde_json::Value =
            serde_json::from_str(&contents).expect("plugin.json must be JSON");

        assert_eq!(
            value["name"], SKILL_DIR_NAME,
            "plugin name must match the installed directory name"
        );
        assert_eq!(
            value["version"],
            env!("CARGO_PKG_VERSION"),
            "plugin version must track the crate version; a stale pin suppresses plugin updates"
        );
        assert!(
            value["agents"].is_null(),
            "no agents key should be needed: agents/openai.yaml is withheld from Claude, so there \
             is no default agents/ folder for a manifest key to override"
        );
        assert_eq!(
            value["skills"].as_array().map(Vec::len),
            Some(1),
            "an explicit skills path keeps the root SKILL.md loading on Claude Code < 2.1.142, \
             where the single-skill-plugin layout is not auto-detected"
        );
        assert!(
            bundled.join("SKILL.md").is_file() && !bundled.join("skills").exists(),
            "single-skill-plugin layout: SKILL.md at root and no skills/ subdirectory"
        );
    }

    /// Lookup keys are `/`-separated regardless of the platform separator.
    #[test]
    fn nested_asset_keys_normalize_across_platform_separators() {
        let nested: PathBuf = [".claude-plugin", "plugin.json"].iter().collect();
        assert_eq!(
            relative_asset_key(&nested).as_deref(),
            Some(".claude-plugin/plugin.json")
        );
        assert_eq!(
            relative_asset_key(Path::new("mcp.json")).as_deref(),
            Some("mcp.json")
        );
    }

    /// A provider-scoped asset must not make the non-owning host churn: the freshly written tree
    /// has to compare equal on the next run even though the source has a file it did not receive.
    #[test]
    fn non_owning_provider_install_converges_without_owned_asset() {
        let root = temp_dir("skill-converge-root");
        let home = temp_dir("skill-converge-home");
        let source = temp_dir("skill-converge-source");
        fs::create_dir_all(&root).expect("root");
        fs::create_dir_all(home.join(".codex/skills")).expect("skills parent");
        fs::create_dir_all(&source).expect("source");
        fs::write(source.join("SKILL.md"), "version=test-skill\n").expect("skill md");
        fs::write(source.join("mcp.json"), "{\"frigg\":{}}\n").expect("skill mcp");

        let plan = plan_skill_install(
            SkillProvider::Codex,
            &root,
            Some(&home),
            Some(&source),
            false,
        );
        assert_eq!(plan.action, SkillInstallAction::Create);
        apply_skill_install(&plan).expect("apply");

        let dest = home.join(".codex/skills").join(SKILL_DIR_NAME);
        assert!(!dest.join("mcp.json").exists());

        let plan2 = plan_skill_install(
            SkillProvider::Codex,
            &root,
            Some(&home),
            Some(&source),
            false,
        );
        assert_eq!(
            plan2.action,
            SkillInstallAction::Unchanged,
            "codex install must converge, not re-Update forever"
        );

        fs::remove_dir_all(root).ok();
        fs::remove_dir_all(home).ok();
        fs::remove_dir_all(source).ok();
    }

    /// A machine with Claude but no skill ever written: `~/.claude` exists, `~/.claude/skills`
    /// does not. Skipping here is what made `--skill-provider claude` quietly do nothing.
    #[test]
    fn creates_the_claude_skills_leaf_when_claude_home_exists() {
        let root = temp_dir("skill-create-root");
        let home = temp_dir("skill-create-home");
        let source = temp_dir("skill-create-source");
        fs::create_dir_all(&root).expect("root");
        fs::create_dir_all(home.join(".claude")).expect("claude home");
        fs::create_dir_all(&source).expect("source");
        fs::write(source.join("SKILL.md"), "version=test-skill\n").expect("skill md");

        let plan = plan_skill_install(
            SkillProvider::Claude,
            &root,
            Some(&home),
            Some(&source),
            false,
        );
        assert_eq!(plan.action, SkillInstallAction::Create, "{}", plan.reason);
        assert_eq!(
            plan.parent_to_create.as_deref(),
            Some(home.join(".claude/skills").as_path()),
            "plan should name the exact leaf it authorizes creating"
        );
        assert!(
            plan.reason.contains("creating-skills-parent"),
            "the mkdir must be visible in the plan reason: {}",
            plan.reason
        );

        apply_skill_install(&plan).expect("apply");
        assert!(
            home.join(".claude/skills")
                .join(SKILL_DIR_NAME)
                .join("SKILL.md")
                .is_file()
        );

        let plan2 = plan_skill_install(
            SkillProvider::Claude,
            &root,
            Some(&home),
            Some(&source),
            false,
        );
        assert_eq!(plan2.action, SkillInstallAction::Unchanged);
        assert!(
            plan2.parent_to_create.is_none(),
            "the leaf now exists, so nothing left to create"
        );

        fs::remove_dir_all(root).ok();
        fs::remove_dir_all(home).ok();
        fs::remove_dir_all(source).ok();
    }

    /// Without `~/.claude`, Claude is not set up on this machine and Frigg must not invent a
    /// config tree for it.
    #[test]
    fn does_not_create_claude_home_itself() {
        let root = temp_dir("skill-nohome-root");
        let home = temp_dir("skill-nohome-home");
        let source = temp_dir("skill-nohome-source");
        fs::create_dir_all(&root).expect("root");
        fs::create_dir_all(&home).expect("home");
        fs::create_dir_all(&source).expect("source");
        fs::write(source.join("SKILL.md"), "version=test-skill\n").expect("skill md");

        let plan = plan_skill_install(
            SkillProvider::Claude,
            &root,
            Some(&home),
            Some(&source),
            false,
        );
        assert_eq!(plan.action, SkillInstallAction::Skipped);
        assert!(!home.join(".claude").exists(), "must not create ~/.claude");

        fs::remove_dir_all(root).ok();
        fs::remove_dir_all(home).ok();
        fs::remove_dir_all(source).ok();
    }

    /// The exception is Claude-only. Other hosts keep the conservative never-create rule.
    #[test]
    fn other_providers_still_never_create_their_skills_parent() {
        let home = temp_dir("skill-other-home");
        fs::create_dir_all(home.join(".codex")).expect("codex home");
        fs::create_dir_all(home.join(".cursor")).expect("cursor home");
        fs::create_dir_all(home.join(".config/amp")).expect("amp home");

        for provider in [
            SkillProvider::Codex,
            SkillProvider::Cursor,
            SkillProvider::Amp,
            SkillProvider::Copilot,
        ] {
            assert!(
                creatable_skills_parent(provider, Some(&home)).is_none(),
                "{} must not create its skills parent",
                provider.as_str()
            );
        }

        fs::remove_dir_all(home).ok();
    }

    /// Uninstall must not create anything: with no parent there is nothing installed to remove.
    #[test]
    fn uninstall_does_not_create_the_claude_skills_leaf() {
        let root = temp_dir("skill-uninst-root");
        let home = temp_dir("skill-uninst-home");
        fs::create_dir_all(&root).expect("root");
        fs::create_dir_all(home.join(".claude")).expect("claude home");

        let plan = plan_skill_install(SkillProvider::Claude, &root, Some(&home), None, true);

        assert_eq!(plan.action, SkillInstallAction::Skipped, "{}", plan.reason);
        assert!(plan.parent_to_create.is_none());
        apply_skill_install(&plan).expect("apply");
        assert!(
            !home.join(".claude/skills").exists(),
            "uninstall must not create the leaf it is about to find empty"
        );

        fs::remove_dir_all(root).ok();
        fs::remove_dir_all(home).ok();
    }

    /// A regular file sitting at `~/.claude/skills` must never be clobbered.
    #[test]
    fn refuses_to_replace_a_file_at_the_claude_skills_path() {
        let root = temp_dir("skill-filepath-root");
        let home = temp_dir("skill-filepath-home");
        let source = temp_dir("skill-filepath-source");
        fs::create_dir_all(&root).expect("root");
        fs::create_dir_all(home.join(".claude")).expect("claude home");
        fs::create_dir_all(&source).expect("source");
        fs::write(source.join("SKILL.md"), "version=test-skill\n").expect("skill md");
        fs::write(home.join(".claude/skills"), "user data").expect("file at skills path");

        let plan = plan_skill_install(
            SkillProvider::Claude,
            &root,
            Some(&home),
            Some(&source),
            false,
        );
        assert_eq!(plan.action, SkillInstallAction::Skipped, "{}", plan.reason);
        assert!(plan.parent_to_create.is_none());
        apply_skill_install(&plan).expect("apply must be a no-op");
        assert_eq!(
            fs::read_to_string(home.join(".claude/skills")).expect("file survives"),
            "user data",
            "the user's file must be untouched"
        );

        fs::remove_dir_all(root).ok();
        fs::remove_dir_all(home).ok();
        fs::remove_dir_all(source).ok();
    }

    /// `exists()` follows symlinks, so a dangling link reads as absent while planning and then
    /// collides at mkdir. That must fail with an actionable message, not a panic or a bare EEXIST.
    #[test]
    fn reports_a_dangling_symlink_at_the_claude_skills_path() {
        let root = temp_dir("skill-symlink-root");
        let home = temp_dir("skill-symlink-home");
        let source = temp_dir("skill-symlink-source");
        fs::create_dir_all(&root).expect("root");
        fs::create_dir_all(home.join(".claude")).expect("claude home");
        fs::create_dir_all(&source).expect("source");
        fs::write(source.join("SKILL.md"), "version=test-skill\n").expect("skill md");
        std::os::unix::fs::symlink(home.join("nowhere"), home.join(".claude/skills"))
            .expect("dangling symlink");

        let plan = plan_skill_install(
            SkillProvider::Claude,
            &root,
            Some(&home),
            Some(&source),
            false,
        );
        let err = apply_skill_install(&plan).expect_err("dangling symlink must fail");
        let message = err.to_string();
        assert!(
            message.contains("symlink"),
            "error should name the symlink so the user can fix it: {message}"
        );

        fs::remove_dir_all(root).ok();
        fs::remove_dir_all(home).ok();
        fs::remove_dir_all(source).ok();
    }

    /// The parent appearing between plan and apply is benign: apply rechecks and proceeds.
    #[test]
    fn tolerates_the_skills_parent_appearing_after_planning() {
        let root = temp_dir("skill-race-root");
        let home = temp_dir("skill-race-home");
        let source = temp_dir("skill-race-source");
        fs::create_dir_all(&root).expect("root");
        fs::create_dir_all(home.join(".claude")).expect("claude home");
        fs::create_dir_all(&source).expect("source");
        fs::write(source.join("SKILL.md"), "version=test-skill\n").expect("skill md");

        let plan = plan_skill_install(
            SkillProvider::Claude,
            &root,
            Some(&home),
            Some(&source),
            false,
        );
        assert!(plan.parent_to_create.is_some());
        fs::create_dir(home.join(".claude/skills")).expect("someone else creates it first");

        apply_skill_install(&plan).expect("apply should tolerate the parent already existing");
        assert!(
            home.join(".claude/skills")
                .join(SKILL_DIR_NAME)
                .join("SKILL.md")
                .is_file()
        );

        fs::remove_dir_all(root).ok();
        fs::remove_dir_all(home).ok();
        fs::remove_dir_all(source).ok();
    }

    #[test]
    fn skips_when_skills_parent_missing() {
        let root = temp_dir("skill-skip-parent");
        fs::create_dir_all(&root).expect("root");
        let home = temp_dir("skill-skip-home");
        fs::create_dir_all(&home).expect("home");

        let plan = plan_skill_install(SkillProvider::Claude, &root, Some(&home), None, false);
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
        fs::write(source.join("mcp.json"), "{\"frigg\":{}}\n").expect("skill mcp");
        fs::write(source.join("references/a.md"), "ref\n").expect("ref");

        let plan = plan_skill_install(
            SkillProvider::Claude,
            &root,
            Some(&home),
            Some(&source),
            false,
        );
        assert_eq!(plan.action, SkillInstallAction::Create);
        apply_skill_install(&plan).expect("apply");

        let dest = home.join(".claude/skills").join(SKILL_DIR_NAME);
        assert!(dest.join("SKILL.md").is_file());
        assert!(
            !dest.join("mcp.json").exists(),
            "mcp.json is Amp's config shape and filename; Claude never reads it"
        );
        assert!(dest.join("references/a.md").is_file());

        let plan2 = plan_skill_install(
            SkillProvider::Claude,
            &root,
            Some(&home),
            Some(&source),
            false,
        );
        assert_eq!(plan2.action, SkillInstallAction::Unchanged);

        fs::write(source.join("references/a.md"), "ref-changed\n").expect("change ref");
        let plan3 = plan_skill_install(
            SkillProvider::Claude,
            &root,
            Some(&home),
            Some(&source),
            false,
        );
        assert_eq!(plan3.action, SkillInstallAction::Update);
        apply_skill_install(&plan3).expect("apply update");
        assert_eq!(
            fs::read_to_string(dest.join("references/a.md")).expect("read dest ref"),
            "ref-changed\n"
        );

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
    fn amp_prefers_standard_global_then_legacy_then_project_skills() {
        let root = temp_dir("skill-amp-root");
        let home = temp_dir("skill-amp-home");
        fs::create_dir_all(root.join(".agents/skills")).expect("project skills");
        fs::create_dir_all(home.join(".config/amp/skills")).expect("legacy skills");
        fs::create_dir_all(home.join(".config/agents/skills")).expect("standard skills");

        let parent = resolve_existing_skills_parent(SkillProvider::Amp, &root, Some(&home));
        assert_eq!(parent, Some(home.join(".config/agents/skills")));

        fs::remove_dir_all(home.join(".config/agents/skills")).expect("remove standard skills");
        let parent = resolve_existing_skills_parent(SkillProvider::Amp, &root, Some(&home));
        assert_eq!(parent, Some(home.join(".config/amp/skills")));

        fs::remove_dir_all(home.join(".config/amp/skills")).expect("remove legacy skills");
        let parent = resolve_existing_skills_parent(SkillProvider::Amp, &root, Some(&home));
        assert_eq!(parent, Some(root.join(".agents/skills")));

        fs::remove_dir_all(root).ok();
        fs::remove_dir_all(home).ok();
    }

    #[test]
    fn never_creates_skills_parent_on_apply_error_path() {
        let root = temp_dir("skill-no-mkdir-root");
        fs::create_dir_all(&root).expect("root");
        let home = temp_dir("skill-no-mkdir-home");
        fs::create_dir_all(&home).expect("home");
        let plan = plan_skill_install(SkillProvider::Claude, &root, Some(&home), None, false);
        assert_eq!(plan.action, SkillInstallAction::Skipped);
        apply_skill_install(&plan).expect("skip is ok");
        assert!(!home.join(".claude").exists());

        fs::remove_dir_all(root).ok();
        fs::remove_dir_all(home).ok();
    }

    /// Unique temp path per call.
    ///
    /// A timestamp alone is not enough: these tests run on parallel threads and the system clock
    /// is coarser than nanoseconds, so two concurrent calls can read the same value and hand two
    /// tests the same directory. The pid and counter make collisions impossible.
    fn temp_dir(stem: &str) -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        let sequence = COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "frigg-{stem}-{}-{unique}-{sequence}",
            std::process::id()
        ))
    }
}
