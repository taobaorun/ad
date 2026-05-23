//! CRUD for the project registry (D9).
//!
//! State stored at `~/.ad/state/projects.json` as a JSON array of `Project`.
//! Path is the primary key; we canonicalize on add to deduplicate symlinks
//! and `~`-relative inputs. Removing a project from the registry does NOT
//! touch the project's filesystem state — `.claude/`, AD backups, history
//! all stay intact (D9, plan §06).

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::Context;
use chrono::Utc;

use crate::fs::atomic::write_atomic;
use crate::fs::paths::{ensure_dir, home, projects_state_path, state_dir};
use crate::models::{Project, ProjectStatus};

use super::{CmdResult, CommandError};

pub(crate) fn load() -> CmdResult<Vec<Project>> {
    let path = projects_state_path()?;
    if !path.exists() {
        return Ok(Vec::new());
    }
    let bytes = std::fs::read(&path).with_context(|| format!("read {}", path.display()))?;
    let projects: Vec<Project> =
        serde_json::from_slice(&bytes).with_context(|| format!("parse {}", path.display()))?;
    Ok(projects)
}

pub(crate) fn save(projects: &[Project]) -> CmdResult<()> {
    ensure_dir(&state_dir()?)?;
    let path = projects_state_path()?;
    let bytes = serde_json::to_vec_pretty(projects)?;
    write_atomic(&path, &bytes)?;
    Ok(())
}

fn expand_tilde(input: &str) -> CmdResult<PathBuf> {
    if let Some(rest) = input.strip_prefix("~/") {
        Ok(home()?.join(rest))
    } else if input == "~" {
        Ok(home()?)
    } else {
        Ok(PathBuf::from(input))
    }
}

fn canonicalize_existing(input: &str) -> CmdResult<PathBuf> {
    let expanded = expand_tilde(input)?;
    if !expanded.exists() {
        return Err(CommandError::Generic(format!(
            "path does not exist: {}",
            expanded.display()
        )));
    }
    if !expanded.is_dir() {
        return Err(CommandError::Generic(format!(
            "not a directory: {}",
            expanded.display()
        )));
    }
    std::fs::canonicalize(&expanded)
        .map_err(|e| CommandError::Generic(format!("canonicalize {}: {}", expanded.display(), e)))
}

#[tauri::command]
pub fn list_projects() -> CmdResult<Vec<Project>> {
    load()
}

#[tauri::command]
pub fn add_project(path: String) -> CmdResult<Project> {
    let canonical = canonicalize_existing(&path)?;
    let canonical_str = canonical.to_string_lossy().into_owned();

    let mut projects = load()?;
    if let Some(idx) = projects.iter().position(|p| p.path == canonical_str) {
        // Idempotent: refresh added_at as a "last seen" pulse.
        projects[idx].added_at = Utc::now();
        let cloned = projects[idx].clone();
        save(&projects)?;
        return Ok(cloned);
    }

    let display_name = canonical
        .file_name()
        .and_then(|s| s.to_str())
        .map(str::to_string)
        .unwrap_or_else(|| canonical_str.clone());

    let project = Project {
        path: canonical_str,
        display_name,
        added_at: Utc::now(),
        current_profile_id: None,
        last_applied: None,
    };
    projects.push(project.clone());
    save(&projects)?;
    Ok(project)
}

#[tauri::command]
pub fn remove_project(path: String) -> CmdResult<()> {
    let mut projects = load()?;
    let target = path_match_target(&path);
    let before = projects.len();
    projects.retain(|p| !target.iter().any(|t| t == &p.path));
    if projects.len() == before {
        return Err(CommandError::Generic(format!("project not found: {path}")));
    }
    save(&projects)?;
    Ok(())
}

#[tauri::command]
pub fn rename_project(path: String, display_name: String) -> CmdResult<Project> {
    if display_name.trim().is_empty() {
        return Err(CommandError::Generic("display_name is empty".into()));
    }
    let mut projects = load()?;
    let target = path_match_target(&path);
    let p = projects
        .iter_mut()
        .find(|p| target.iter().any(|t| t == &p.path))
        .ok_or_else(|| CommandError::Generic(format!("project not found: {path}")))?;
    p.display_name = display_name;
    let updated = p.clone();
    save(&projects)?;
    Ok(updated)
}

/// Returns the candidate stored-path strings to match against, given a
/// user-supplied input path (raw / tilde / canonicalized).
fn path_match_target(input: &str) -> Vec<String> {
    let mut out = vec![input.to_string()];
    if let Ok(expanded) = expand_tilde(input) {
        out.push(expanded.to_string_lossy().into_owned());
        if let Ok(canonical) = std::fs::canonicalize(&expanded) {
            out.push(canonical.to_string_lossy().into_owned());
        }
    }
    out
}

#[tauri::command]
pub fn get_project_status(path: String) -> CmdResult<ProjectStatus> {
    let target = expand_tilde(&path)?;
    let exists = target.exists();
    if !exists {
        return Ok(ProjectStatus {
            exists: false,
            is_git_repo: false,
            git_dirty: false,
            claude_dir_exists: false,
            has_settings_json: false,
            has_settings_local_json: false,
            gitignore_excludes_settings_local: None,
        });
    }

    let is_git_repo = target.join(".git").exists();
    let git_dirty = if is_git_repo {
        git_working_tree_dirty(&target)
    } else {
        false
    };
    let claude_dir = target.join(".claude");
    let claude_dir_exists = claude_dir.exists();
    let has_settings_json = claude_dir.join("settings.json").exists();
    let has_settings_local_json = claude_dir.join("settings.local.json").exists();

    let gitignore_excludes_settings_local = if is_git_repo {
        Some(gitignore_excludes_settings_local(&target))
    } else {
        None
    };

    Ok(ProjectStatus {
        exists: true,
        is_git_repo,
        git_dirty,
        claude_dir_exists,
        has_settings_json,
        has_settings_local_json,
        gitignore_excludes_settings_local,
    })
}

/// Runs `git status --porcelain` in `dir`. Empty output ⇒ clean. Anything else
/// (including failure to invoke git) ⇒ treat as dirty/unknown — the UI will
/// warn the user before letting them apply to shared.
fn git_working_tree_dirty(dir: &Path) -> bool {
    match Command::new("git")
        .current_dir(dir)
        .args(["status", "--porcelain"])
        .output()
    {
        Ok(out) if out.status.success() => !out.stdout.is_empty(),
        _ => true, // err on the side of caution
    }
}

/// Naive substring check on `.gitignore` for `settings.local.json`. Doesn't
/// parse gitignore syntax — just looks for the literal pattern. Good enough
/// for the warning UI; user can override.
fn gitignore_excludes_settings_local(dir: &Path) -> bool {
    let candidates = [dir.join(".gitignore"), dir.join(".claude/.gitignore")];
    for path in candidates {
        if let Ok(content) = std::fs::read_to_string(&path) {
            for line in content.lines() {
                let line = line.trim();
                if line.starts_with('#') || line.is_empty() {
                    continue;
                }
                if line == "settings.local.json"
                    || line == ".claude/settings.local.json"
                    || line == "**/settings.local.json"
                {
                    return true;
                }
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use tempfile::TempDir;

    fn setup_home() -> TempDir {
        let tmp = TempDir::new().unwrap();
        std::env::set_var("AD_HOME", tmp.path());
        tmp
    }

    fn make_project_dir(home: &Path, name: &str) -> PathBuf {
        let p = home.join(name);
        std::fs::create_dir(&p).unwrap();
        p
    }

    #[test]
    #[serial(home_env)]
    fn list_empty_when_no_state() {
        let _g = setup_home();
        assert!(list_projects().unwrap().is_empty());
    }

    #[test]
    #[serial(home_env)]
    fn add_creates_canonical_entry_with_basename_display() {
        let g = setup_home();
        let p = make_project_dir(g.path(), "foo");

        let project = add_project(p.to_string_lossy().into_owned()).unwrap();
        assert_eq!(project.display_name, "foo");
        let canonical = std::fs::canonicalize(&p).unwrap();
        assert_eq!(project.path, canonical.to_string_lossy());

        let listed = list_projects().unwrap();
        assert_eq!(listed.len(), 1);
        assert!(listed[0].current_profile_id.is_none());
        assert!(listed[0].last_applied.is_none());
    }

    #[test]
    #[serial(home_env)]
    fn add_is_idempotent() {
        let g = setup_home();
        let p = make_project_dir(g.path(), "foo");
        let s = p.to_string_lossy().into_owned();
        add_project(s.clone()).unwrap();
        add_project(s).unwrap();
        assert_eq!(list_projects().unwrap().len(), 1);
    }

    #[test]
    #[serial(home_env)]
    fn add_rejects_nonexistent_or_file() {
        let g = setup_home();
        let f = g.path().join("file.txt");
        std::fs::write(&f, b"x").unwrap();
        assert!(add_project(f.to_string_lossy().into_owned()).is_err());
        assert!(add_project("/nope/nope/nope".into()).is_err());
    }

    #[test]
    #[serial(home_env)]
    fn remove_works_with_tilde_input() {
        let g = setup_home();
        let p = make_project_dir(g.path(), "foo");
        add_project(p.to_string_lossy().into_owned()).unwrap();
        remove_project("~/foo".into()).unwrap();
        assert!(list_projects().unwrap().is_empty());
    }

    #[test]
    #[serial(home_env)]
    fn rename_changes_display_name_only() {
        let g = setup_home();
        let p = make_project_dir(g.path(), "foo");
        add_project(p.to_string_lossy().into_owned()).unwrap();
        let renamed = rename_project("~/foo".into(), "Foo Project".into()).unwrap();
        assert_eq!(renamed.display_name, "Foo Project");
        // Path unchanged.
        let canonical = std::fs::canonicalize(&p).unwrap();
        assert_eq!(renamed.path, canonical.to_string_lossy());
    }

    #[test]
    #[serial(home_env)]
    fn rename_rejects_empty_name() {
        let g = setup_home();
        let p = make_project_dir(g.path(), "foo");
        add_project(p.to_string_lossy().into_owned()).unwrap();
        assert!(rename_project("~/foo".into(), "   ".into()).is_err());
    }

    #[test]
    #[serial(home_env)]
    fn status_for_nonexistent_path_returns_all_false() {
        let _g = setup_home();
        let s = get_project_status("/definitely/missing/xyz".into()).unwrap();
        assert!(!s.exists);
        assert!(!s.is_git_repo);
    }

    #[test]
    #[serial(home_env)]
    fn status_detects_git_and_claude_dir() {
        let g = setup_home();
        let p = make_project_dir(g.path(), "proj");
        std::fs::create_dir(p.join(".git")).unwrap();
        std::fs::create_dir(p.join(".claude")).unwrap();
        std::fs::write(p.join(".claude/settings.local.json"), b"{}").unwrap();

        let s = get_project_status(p.to_string_lossy().into_owned()).unwrap();
        assert!(s.exists);
        assert!(s.is_git_repo);
        assert!(s.claude_dir_exists);
        assert!(!s.has_settings_json);
        assert!(s.has_settings_local_json);
    }

    #[test]
    #[serial(home_env)]
    fn status_reads_gitignore_for_local_json_exclusion() {
        let g = setup_home();
        let p = make_project_dir(g.path(), "proj");
        std::fs::create_dir(p.join(".git")).unwrap();
        std::fs::write(
            p.join(".gitignore"),
            "node_modules\n.claude/settings.local.json\n",
        )
        .unwrap();

        let s = get_project_status(p.to_string_lossy().into_owned()).unwrap();
        assert_eq!(s.gitignore_excludes_settings_local, Some(true));
    }
}
