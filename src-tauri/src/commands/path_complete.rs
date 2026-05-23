//! Tab-completion backend for the AddProjectInput component (D12).
//!
//! Given a prefix typed by the user, return up to 50 directory paths that
//! start with that prefix. Frontend renders these in a live dropdown and
//! also handles Tab-key common-prefix completion.
//!
//! Conventions:
//! - Leading `~` expanded via `paths::home()` (respects `AD_HOME` in tests).
//! - Only directories returned (not regular files).
//! - Hidden entries (starting with `.`) skipped unless the *partial* segment
//!   the user is typing starts with `.`.
//! - Returned paths are absolute and end with `/` so the user can keep
//!   typing a sub-segment.

use std::path::{Path, PathBuf};

use crate::fs::paths::home;

use super::{CmdResult, CommandError};

const MAX_COMPLETIONS: usize = 50;

#[tauri::command]
pub fn complete_path_prefix(prefix: String) -> CmdResult<Vec<String>> {
    if prefix.is_empty() {
        return Ok(Vec::new());
    }

    let expanded = expand_tilde(&prefix)?;
    let (parent, partial) = split_parent_and_partial(&expanded)?;

    let entries = match std::fs::read_dir(&parent) {
        Ok(it) => it,
        // Permission denied / not a dir / etc. — return empty silently.
        Err(_) => return Ok(Vec::new()),
    };

    let show_hidden = partial.starts_with('.');
    let mut hits: Vec<String> = Vec::new();
    for entry in entries.flatten() {
        let p = entry.path();
        if !p.is_dir() {
            continue;
        }
        let Some(name) = p.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        if !show_hidden && name.starts_with('.') {
            continue;
        }
        if !name.starts_with(&partial) {
            continue;
        }

        // Always present absolute, with trailing slash so further typing
        // descends into the dir naturally.
        let mut abs = p.to_string_lossy().into_owned();
        if !abs.ends_with('/') {
            abs.push('/');
        }
        hits.push(abs);
        if hits.len() >= MAX_COMPLETIONS {
            break;
        }
    }
    hits.sort();
    Ok(hits)
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

/// Splits an input path into (parent_dir, partial_segment_being_typed).
///
/// Examples:
/// - `/tmp/foo`         → (`/tmp`, `foo`)
/// - `/tmp/`            → (`/tmp`, `""`)
/// - `/tmp`             → (`/`, `tmp`)
/// - `relative/path`    → (`relative`, `path`)  — caller's CWD applies
fn split_parent_and_partial(p: &Path) -> CmdResult<(PathBuf, String)> {
    let s = p.to_string_lossy();
    if s.ends_with('/') {
        return Ok((p.to_path_buf(), String::new()));
    }
    let parent = p
        .parent()
        .ok_or_else(|| CommandError::Generic(format!("invalid path: {}", p.display())))?
        .to_path_buf();
    let partial = p
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string();
    Ok((parent, partial))
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

    #[test]
    fn empty_prefix_returns_nothing() {
        let r = complete_path_prefix("".into()).unwrap();
        assert!(r.is_empty());
    }

    #[test]
    fn lists_subdirs_with_partial_match() {
        let tmp = TempDir::new().unwrap();
        for name in ["foo", "foobar", "bar"] {
            std::fs::create_dir(tmp.path().join(name)).unwrap();
        }
        let prefix = format!("{}/foo", tmp.path().display());
        let r = complete_path_prefix(prefix).unwrap();
        assert_eq!(r.len(), 2, "got: {r:?}");
        assert!(r.iter().any(|p| p.ends_with("/foo/")));
        assert!(r.iter().any(|p| p.ends_with("/foobar/")));
        assert!(!r.iter().any(|p| p.ends_with("/bar/")));
    }

    #[test]
    fn trailing_slash_lists_all_subdirs() {
        let tmp = TempDir::new().unwrap();
        for name in ["a", "b", "c"] {
            std::fs::create_dir(tmp.path().join(name)).unwrap();
        }
        let prefix = format!("{}/", tmp.path().display());
        let r = complete_path_prefix(prefix).unwrap();
        assert_eq!(r.len(), 3);
    }

    #[test]
    fn skips_files_returns_dirs_only() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir(tmp.path().join("dir1")).unwrap();
        std::fs::write(tmp.path().join("file.txt"), b"x").unwrap();
        let prefix = format!("{}/", tmp.path().display());
        let r = complete_path_prefix(prefix).unwrap();
        assert_eq!(r.len(), 1, "got: {r:?}");
        assert!(r[0].ends_with("/dir1/"));
    }

    #[test]
    fn hidden_skipped_unless_partial_starts_with_dot() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir(tmp.path().join("visible")).unwrap();
        std::fs::create_dir(tmp.path().join(".hidden")).unwrap();

        // No dot in partial → hidden skipped.
        let prefix = format!("{}/", tmp.path().display());
        let r = complete_path_prefix(prefix).unwrap();
        assert_eq!(r.len(), 1);
        assert!(r[0].ends_with("/visible/"));

        // Partial starts with `.` → hidden included.
        let prefix = format!("{}/.h", tmp.path().display());
        let r2 = complete_path_prefix(prefix).unwrap();
        assert_eq!(r2.len(), 1);
        assert!(r2[0].ends_with("/.hidden/"));
    }

    #[test]
    #[serial(home_env)]
    fn tilde_expands_to_ad_home_in_tests() {
        let g = setup_home();
        std::fs::create_dir(g.path().join("dev")).unwrap();
        std::fs::create_dir(g.path().join("dot")).unwrap();

        let r = complete_path_prefix("~/d".into()).unwrap();
        assert_eq!(r.len(), 2);
        assert!(r.iter().any(|p| p.ends_with("/dev/")));
        assert!(r.iter().any(|p| p.ends_with("/dot/")));
    }

    #[test]
    fn nonexistent_parent_dir_returns_empty() {
        let r = complete_path_prefix("/definitely/not/here/x".into()).unwrap();
        assert!(r.is_empty());
    }

    #[test]
    fn caps_at_max_completions() {
        let tmp = TempDir::new().unwrap();
        for i in 0..(MAX_COMPLETIONS + 5) {
            std::fs::create_dir(tmp.path().join(format!("dir{i:02}"))).unwrap();
        }
        let prefix = format!("{}/dir", tmp.path().display());
        let r = complete_path_prefix(prefix).unwrap();
        assert_eq!(r.len(), MAX_COMPLETIONS);
    }
}
