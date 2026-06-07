//! Resolves the canonical filesystem locations ad reads and writes.
//!
//! All paths are derived from `$HOME` via `dirs::home_dir()`. We allow the home
//! directory to be overridden in tests via the `AD_HOME` environment
//! variable so that integration tests can run against a `tempfile::TempDir`.
//!
//! AD's own data lives under `~/.ad/` (separate from `~/.claude/`, which
//! belongs to Claude Code). The two roots are independent so AD can be
//! removed cleanly without touching CC state.

use std::path::PathBuf;

use super::FsError;

/// User's home directory. Honors the `AD_HOME` env var override (used by
/// integration tests with `tempfile::TempDir`).
pub fn home() -> Result<PathBuf, FsError> {
    if let Ok(override_home) = std::env::var("AD_HOME") {
        return Ok(PathBuf::from(override_home));
    }
    dirs::home_dir().ok_or(FsError::NoHome)
}

// --- Claude Code's locations (AD only reads ~/.claude/projects for auto-detect; only writes settings.json via the legacy global-overwrite path) ---

pub fn claude_dir() -> Result<PathBuf, FsError> {
    Ok(home()?.join(".claude"))
}

pub fn claude_settings_path() -> Result<PathBuf, FsError> {
    Ok(claude_dir()?.join("settings.json"))
}

/// CC's per-project metadata directory. AD reads this for auto-detect.
pub fn cc_projects_dir() -> Result<PathBuf, FsError> {
    Ok(claude_dir()?.join("projects"))
}

// --- AD's own data root ---

/// AD's data home. Replaces the v0.1 `~/.claude/ad/` location.
pub fn ad_home() -> Result<PathBuf, FsError> {
    Ok(home()?.join(".ad"))
}

pub fn profiles_dir() -> Result<PathBuf, FsError> {
    Ok(ad_home()?.join("profiles"))
}

pub fn legacy_dir() -> Result<PathBuf, FsError> {
    Ok(profiles_dir()?.join(".legacy"))
}

pub fn backups_dir() -> Result<PathBuf, FsError> {
    Ok(ad_home()?.join("backups"))
}

/// Legacy single-file history; still read for backwards compatibility but no
/// longer written.
pub fn history_path() -> Result<PathBuf, FsError> {
    Ok(ad_home()?.join("history.jsonl"))
}

/// Per-activation log directory: each activation writes one
/// `<ts>.<uuid>.json` file via `write_atomic`. Crash-safe by construction.
pub fn history_dir() -> Result<PathBuf, FsError> {
    Ok(ad_home()?.join("history"))
}

pub fn active_pointer_path() -> Result<PathBuf, FsError> {
    Ok(ad_home()?.join("active"))
}

pub fn state_dir() -> Result<PathBuf, FsError> {
    Ok(ad_home()?.join("state"))
}

pub fn projects_state_path() -> Result<PathBuf, FsError> {
    Ok(state_dir()?.join("projects.json"))
}

pub fn scan_roots_state_path() -> Result<PathBuf, FsError> {
    Ok(state_dir()?.join("scan_roots.json"))
}

pub fn theme_hint_path() -> Result<PathBuf, FsError> {
    Ok(state_dir()?.join("theme"))
}

// --- Skill management (v0.5) ---

pub fn skill_library_dir() -> Result<PathBuf, FsError> {
    Ok(ad_home()?.join("skill-library"))
}

pub fn skill_sources_path() -> Result<PathBuf, FsError> {
    Ok(state_dir()?.join("skill_sources.json"))
}

pub fn project_skills_dir() -> Result<PathBuf, FsError> {
    Ok(state_dir()?.join("project_skills"))
}

pub fn claude_skills_dir() -> Result<PathBuf, FsError> {
    Ok(claude_dir()?.join("skills"))
}

// --- Legacy v0.1 paths (only referenced by migrate_data_dir_to_home) ---

/// v0.1 location of AD's data (under ~/.claude/ad/). Migrated to `ad_home()`.
pub fn legacy_v1_ad_dir() -> Result<PathBuf, FsError> {
    Ok(claude_dir()?.join("ad"))
}

/// v0.1 location of profiles (under ~/.claude/profiles/). Migrated to `profiles_dir()`.
pub fn legacy_v1_profiles_dir() -> Result<PathBuf, FsError> {
    Ok(claude_dir()?.join("profiles"))
}

// --- Helpers ---

pub fn ensure_dir(path: &std::path::Path) -> Result<(), FsError> {
    std::fs::create_dir_all(path).map_err(|e| FsError::io(path.display().to_string(), e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn with_temp_home<R>(f: impl FnOnce(&std::path::Path) -> R) -> R {
        let tmp = TempDir::new().expect("temp");
        let prev = std::env::var("AD_HOME").ok();
        std::env::set_var("AD_HOME", tmp.path());
        let r = f(tmp.path());
        match prev {
            Some(v) => std::env::set_var("AD_HOME", v),
            None => std::env::remove_var("AD_HOME"),
        }
        r
    }

    #[test]
    #[serial_test::serial(home_env)]
    fn paths_resolve_under_home_override() {
        with_temp_home(|home| {
            // CC's locations stay under ~/.claude/
            assert_eq!(claude_dir().unwrap(), home.join(".claude"));
            assert_eq!(
                claude_settings_path().unwrap(),
                home.join(".claude/settings.json")
            );
            assert_eq!(cc_projects_dir().unwrap(), home.join(".claude/projects"));

            // AD's own data lives under ~/.ad/
            assert_eq!(ad_home().unwrap(), home.join(".ad"));
            assert_eq!(profiles_dir().unwrap(), home.join(".ad/profiles"));
            assert_eq!(legacy_dir().unwrap(), home.join(".ad/profiles/.legacy"));
            assert_eq!(backups_dir().unwrap(), home.join(".ad/backups"));
            assert_eq!(history_path().unwrap(), home.join(".ad/history.jsonl"));
            assert_eq!(history_dir().unwrap(), home.join(".ad/history"));
            assert_eq!(active_pointer_path().unwrap(), home.join(".ad/active"));
            assert_eq!(state_dir().unwrap(), home.join(".ad/state"));
            assert_eq!(
                projects_state_path().unwrap(),
                home.join(".ad/state/projects.json")
            );
            assert_eq!(
                scan_roots_state_path().unwrap(),
                home.join(".ad/state/scan_roots.json")
            );
            // v0.1 legacy locations (used by migration only)
            assert_eq!(legacy_v1_ad_dir().unwrap(), home.join(".claude/ad"));
            assert_eq!(
                legacy_v1_profiles_dir().unwrap(),
                home.join(".claude/profiles")
            );
        });
    }

    #[test]
    #[serial_test::serial(home_env)]
    fn ensure_dir_creates_nested() {
        with_temp_home(|home| {
            let nested = home.join("a/b/c");
            ensure_dir(&nested).unwrap();
            assert!(nested.is_dir());
        });
    }
}
