//! Resolves the canonical filesystem locations cc-switch reads and writes.
//!
//! All paths are derived from `$HOME` via `dirs::home_dir()`. We allow the home
//! directory to be overridden in tests via the `CC_SWITCH_HOME` environment
//! variable so that integration tests can run against a `tempfile::TempDir`.

use std::path::PathBuf;

use super::FsError;

fn home() -> Result<PathBuf, FsError> {
    if let Ok(override_home) = std::env::var("CC_SWITCH_HOME") {
        return Ok(PathBuf::from(override_home));
    }
    dirs::home_dir().ok_or(FsError::NoHome)
}

pub fn claude_dir() -> Result<PathBuf, FsError> {
    Ok(home()?.join(".claude"))
}

pub fn profiles_dir() -> Result<PathBuf, FsError> {
    Ok(claude_dir()?.join("profiles"))
}

pub fn legacy_dir() -> Result<PathBuf, FsError> {
    Ok(profiles_dir()?.join(".legacy"))
}

pub fn cc_switch_dir() -> Result<PathBuf, FsError> {
    Ok(claude_dir()?.join("cc-switch"))
}

pub fn backups_dir() -> Result<PathBuf, FsError> {
    Ok(cc_switch_dir()?.join("backups"))
}

/// Legacy single-file history; still read for backwards compatibility but no
/// longer written.
pub fn history_path() -> Result<PathBuf, FsError> {
    Ok(cc_switch_dir()?.join("history.jsonl"))
}

/// Per-activation log directory: each activation writes one
/// `<ts>.<uuid>.json` file via `write_atomic`. Crash-safe by construction.
pub fn history_dir() -> Result<PathBuf, FsError> {
    Ok(cc_switch_dir()?.join("history"))
}

pub fn active_pointer_path() -> Result<PathBuf, FsError> {
    Ok(cc_switch_dir()?.join("active"))
}

pub fn claude_settings_path() -> Result<PathBuf, FsError> {
    Ok(claude_dir()?.join("settings.json"))
}

pub fn ensure_dir(path: &std::path::Path) -> Result<(), FsError> {
    std::fs::create_dir_all(path).map_err(|e| FsError::io(path.display().to_string(), e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn with_temp_home<R>(f: impl FnOnce(&std::path::Path) -> R) -> R {
        let tmp = TempDir::new().expect("temp");
        let prev = std::env::var("CC_SWITCH_HOME").ok();
        std::env::set_var("CC_SWITCH_HOME", tmp.path());
        let r = f(tmp.path());
        match prev {
            Some(v) => std::env::set_var("CC_SWITCH_HOME", v),
            None => std::env::remove_var("CC_SWITCH_HOME"),
        }
        r
    }

    #[test]
    #[serial_test::serial(home_env)]
    fn paths_resolve_under_home_override() {
        with_temp_home(|home| {
            assert_eq!(claude_dir().unwrap(), home.join(".claude"));
            assert_eq!(profiles_dir().unwrap(), home.join(".claude/profiles"));
            assert_eq!(legacy_dir().unwrap(), home.join(".claude/profiles/.legacy"));
            assert_eq!(
                backups_dir().unwrap(),
                home.join(".claude/cc-switch/backups")
            );
            assert_eq!(
                history_path().unwrap(),
                home.join(".claude/cc-switch/history.jsonl")
            );
            assert_eq!(
                claude_settings_path().unwrap(),
                home.join(".claude/settings.json")
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
