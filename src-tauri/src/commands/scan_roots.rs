//! CRUD for the scan-roots config (D12).
//!
//! Stored at `~/.ad/state/scan_roots.json`. The default state contains a
//! single builtin entry pointing at `~/.claude/projects` (CC's per-project
//! metadata, kind = `cc_projects_meta`). Users can append `generic` roots
//! that AD walks one level deep when scanning for projects.
//!
//! Path canonicalization: generic roots are canonicalized on add. Builtin
//! roots are stored as the absolute path resolved at first read.

use std::path::{Path, PathBuf};

use anyhow::Context;

use crate::fs::atomic::write_atomic;
use crate::fs::paths::{claude_dir, ensure_dir, home, scan_roots_state_path, state_dir};
use crate::models::{ScanRoot, ScanRootKind};

use super::{CmdResult, CommandError};

/// Reads the scan_roots state file. If it doesn't exist yet, returns the
/// default (one builtin entry for CC's projects dir). Does **not** persist
/// the default — callers that mutate state should always go through `save`.
pub(crate) fn load() -> CmdResult<Vec<ScanRoot>> {
    let path = scan_roots_state_path()?;
    if !path.exists() {
        return Ok(vec![default_builtin()?]);
    }
    let bytes = std::fs::read(&path).with_context(|| format!("read {}", path.display()))?;
    let roots: Vec<ScanRoot> =
        serde_json::from_slice(&bytes).with_context(|| format!("parse {}", path.display()))?;
    Ok(roots)
}

fn save(roots: &[ScanRoot]) -> CmdResult<()> {
    let dir = state_dir()?;
    ensure_dir(&dir)?;
    let path = scan_roots_state_path()?;
    let bytes = serde_json::to_vec_pretty(roots)?;
    write_atomic(&path, &bytes)?;
    Ok(())
}

fn default_builtin() -> CmdResult<ScanRoot> {
    let path = claude_dir()?.join("projects");
    Ok(ScanRoot {
        path: path.to_string_lossy().into_owned(),
        kind: ScanRootKind::CcProjectsMeta,
        builtin: true,
        enabled: true,
    })
}

/// Canonicalizes a user-supplied path: expands a leading `~`, then resolves
/// symlinks via `canonicalize` if the path exists. If the path doesn't exist
/// yet, returns the absolute form without canonicalization.
fn canonicalize_user_path(input: &str) -> CmdResult<PathBuf> {
    let expanded = expand_tilde(input)?;
    if expanded.exists() {
        std::fs::canonicalize(&expanded).map_err(|e| {
            CommandError::Generic(format!("canonicalize {}: {}", expanded.display(), e))
        })
    } else {
        Err(CommandError::Generic(format!(
            "path does not exist: {}",
            expanded.display()
        )))
    }
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

#[tauri::command]
pub fn list_scan_roots() -> CmdResult<Vec<ScanRoot>> {
    load()
}

/// Adds a `generic` scan root. Returns the resulting list (after dedup).
#[tauri::command]
pub fn add_scan_root(path: String) -> CmdResult<Vec<ScanRoot>> {
    let canonical = canonicalize_user_path(&path)?;
    if !canonical.is_dir() {
        return Err(CommandError::Generic(format!(
            "not a directory: {}",
            canonical.display()
        )));
    }
    let canonical_str = canonical.to_string_lossy().into_owned();

    let mut roots = load()?;
    if roots.iter().any(|r| r.path == canonical_str) {
        // Idempotent dedup.
        return Ok(roots);
    }
    roots.push(ScanRoot {
        path: canonical_str,
        kind: ScanRootKind::Generic,
        builtin: false,
        enabled: true,
    });
    save(&roots)?;
    Ok(roots)
}

/// Returns true if the input path matches the stored root path. Inputs may be
/// raw user input (tilde, non-canonical) so we try a few normalizations.
fn path_matches(stored: &str, input: &str) -> bool {
    if stored == input {
        return true;
    }
    if let Ok(expanded) = expand_tilde(input) {
        if stored == expanded.to_string_lossy() {
            return true;
        }
        // Also try canonicalize (resolves /var → /private/var on macOS).
        if let Ok(canonical) = std::fs::canonicalize(&expanded) {
            if stored == canonical.to_string_lossy() {
                return true;
            }
        }
    }
    false
}

/// Removes a non-builtin scan root by path. Builtin roots can only be toggled
/// via `set_scan_root_enabled`.
#[tauri::command]
pub fn remove_scan_root(path: String) -> CmdResult<Vec<ScanRoot>> {
    let mut roots = load()?;
    let pos = roots
        .iter()
        .position(|r| path_matches(&r.path, &path))
        .ok_or_else(|| CommandError::Generic(format!("scan root not found: {path}")))?;
    if roots[pos].builtin {
        return Err(CommandError::Generic(
            "cannot remove builtin scan root; toggle it via set_scan_root_enabled instead".into(),
        ));
    }
    roots.remove(pos);
    save(&roots)?;
    Ok(roots)
}

/// Enables/disables a scan root by path. Works on both builtin and user roots.
#[tauri::command]
pub fn set_scan_root_enabled(path: String, enabled: bool) -> CmdResult<Vec<ScanRoot>> {
    let mut roots = load()?;
    let r = roots
        .iter_mut()
        .find(|r| path_matches(&r.path, &path))
        .ok_or_else(|| CommandError::Generic(format!("scan root not found: {path}")))?;
    r.enabled = enabled;
    save(&roots)?;
    Ok(roots)
}

/// Returns only the enabled scan roots, used by `discover` to decide what to
/// walk. Not exposed as a Tauri command (internal).
pub(crate) fn enabled_roots() -> CmdResult<Vec<ScanRoot>> {
    Ok(load()?.into_iter().filter(|r| r.enabled).collect())
}

#[allow(dead_code)]
pub(crate) fn path_eq(a: &Path, b: &str) -> bool {
    a.to_string_lossy() == b
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
    #[serial(home_env)]
    fn list_returns_default_when_no_state_file() {
        let _g = setup_home();
        let roots = list_scan_roots().unwrap();
        assert_eq!(roots.len(), 1);
        assert!(roots[0].builtin);
        assert!(matches!(roots[0].kind, ScanRootKind::CcProjectsMeta));
        assert!(roots[0].path.ends_with(".claude/projects"));
        // Listing without mutating must not write the state file.
        assert!(!scan_roots_state_path().unwrap().exists());
    }

    #[test]
    #[serial(home_env)]
    fn add_persists_canonical_generic_root() {
        let g = setup_home();
        let dev = g.path().join("dev");
        std::fs::create_dir(&dev).unwrap();

        let roots = add_scan_root(dev.to_string_lossy().into_owned()).unwrap();
        assert_eq!(roots.len(), 2);
        assert!(roots
            .iter()
            .any(|r| matches!(r.kind, ScanRootKind::Generic)));
        // State file written.
        assert!(scan_roots_state_path().unwrap().exists());
    }

    #[test]
    #[serial(home_env)]
    fn add_is_idempotent() {
        let g = setup_home();
        let dev = g.path().join("dev");
        std::fs::create_dir(&dev).unwrap();

        let s = dev.to_string_lossy().into_owned();
        let r1 = add_scan_root(s.clone()).unwrap();
        let r2 = add_scan_root(s).unwrap();
        assert_eq!(r1.len(), r2.len());
    }

    #[test]
    #[serial(home_env)]
    fn add_rejects_nonexistent_path() {
        let _g = setup_home();
        let err = add_scan_root("/definitely/does/not/exist".into()).unwrap_err();
        assert!(format!("{err}").contains("does not exist"));
    }

    #[test]
    #[serial(home_env)]
    fn remove_works_on_user_roots_and_rejects_builtin() {
        let g = setup_home();
        let dev = g.path().join("dev");
        std::fs::create_dir(&dev).unwrap();
        let dev_s = dev.to_string_lossy().into_owned();

        add_scan_root(dev_s.clone()).unwrap();
        let after_remove = remove_scan_root(dev_s).unwrap();
        assert_eq!(after_remove.len(), 1);
        assert!(after_remove[0].builtin);

        // Builtin can't be removed.
        let builtin_path = after_remove[0].path.clone();
        let err = remove_scan_root(builtin_path).unwrap_err();
        assert!(
            format!("{err}").contains("builtin"),
            "expected builtin error, got: {err}"
        );
    }

    #[test]
    #[serial(home_env)]
    fn toggle_enabled_works_on_builtin() {
        let _g = setup_home();
        let roots = list_scan_roots().unwrap();
        let builtin_path = roots[0].path.clone();
        let after = set_scan_root_enabled(builtin_path.clone(), false).unwrap();
        assert!(!after[0].enabled);

        // enabled_roots() filters out disabled.
        let enabled = enabled_roots().unwrap();
        assert!(enabled.is_empty());
    }

    #[test]
    #[serial(home_env)]
    fn tilde_expansion_in_remove_path() {
        let g = setup_home();
        // Put a generic root at $AD_HOME/dev (which is the home for the test).
        let dev = g.path().join("dev");
        std::fs::create_dir(&dev).unwrap();
        add_scan_root(dev.to_string_lossy().into_owned()).unwrap();

        // Now invoke remove_scan_root with "~/dev" — it should expand and match.
        let after = remove_scan_root("~/dev".into()).unwrap();
        assert_eq!(after.len(), 1, "user root should be removed");
    }
}
