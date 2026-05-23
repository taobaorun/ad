//! Project auto-detection across configured scan roots (D12).
//!
//! For each enabled `ScanRoot`:
//! - **`cc_projects_meta`** kind (only `~/.claude/projects/`): each subdir is
//!   an encoded project path. Since CC's encoding is lossy (it replaces `/`,
//!   `_`, and `.` with `-`), we recover the original path by reading the
//!   first available `.jsonl` session file inside and parsing its `cwd`
//!   field. This is exactly the path CC was running in when the session
//!   started.
//! - **`generic`** kind: walk one level deep. Any subdir containing `.git/`
//!   or `.claude/` becomes a candidate.
//!
//! Candidates are deduped by canonical path across all roots, and each is
//! annotated with `already_added` based on `~/.ad/state/projects.json`.

use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use anyhow::Context;

use crate::fs::paths::projects_state_path;
use crate::models::{DetectedProject, Project, ScanRoot, ScanRootKind};

use super::scan_roots;
use super::CmdResult;

/// Scans every enabled scan root and returns deduped candidates.
#[tauri::command]
pub fn scan_for_projects() -> CmdResult<Vec<DetectedProject>> {
    let roots = scan_roots::enabled_roots()?;
    let known: HashSet<String> = load_known_project_paths()?;
    let mut results: BTreeMap<String, DetectedProject> = BTreeMap::new();

    for root in roots {
        match root.kind {
            ScanRootKind::CcProjectsMeta => {
                scan_cc_meta(&root, &known, &mut results);
            }
            ScanRootKind::Generic => {
                scan_generic(&root, &known, &mut results);
            }
        }
    }

    Ok(results.into_values().collect())
}

fn load_known_project_paths() -> CmdResult<HashSet<String>> {
    let path = projects_state_path()?;
    if !path.exists() {
        return Ok(HashSet::new());
    }
    let bytes = std::fs::read(&path).with_context(|| format!("read {}", path.display()))?;
    // Tolerate malformed state — corrupted projects.json shouldn't crash discovery.
    let projects: Vec<Project> = serde_json::from_slice(&bytes).unwrap_or_default();
    Ok(projects.into_iter().map(|p| p.path).collect())
}

fn scan_cc_meta(
    root: &ScanRoot,
    known: &HashSet<String>,
    out: &mut BTreeMap<String, DetectedProject>,
) {
    let root_path = Path::new(&root.path);
    let entries = match std::fs::read_dir(root_path) {
        Ok(it) => it,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let p = entry.path();
        if !p.is_dir() {
            continue;
        }
        let Some(cwd) = recover_cwd_from_session(&p) else {
            continue;
        };
        // The recovered cwd may no longer exist on disk (project moved).
        // Still surface it so the user can decide; mark with a "missing" tag.
        let canonical = std::fs::canonicalize(&cwd)
            .map(|c| c.to_string_lossy().into_owned())
            .unwrap_or_else(|_| cwd.clone());

        let mut signals = vec!["cc-history".to_string()];
        if !Path::new(&canonical).exists() {
            signals.push("missing".to_string());
        }
        out.entry(canonical.clone())
            .and_modify(|d| {
                if !d.signals.contains(&"cc-history".to_string()) {
                    d.signals.push("cc-history".to_string());
                }
            })
            .or_insert(DetectedProject {
                already_added: known.contains(&canonical),
                path: canonical,
                source_root: root.path.clone(),
                source_kind: root.kind,
                signals,
            });
    }
}

/// Tries to find any `*.jsonl` file under `dir` and extract a `cwd` field
/// from any JSON line within. Returns None if no usable file is found.
/// Stops scanning a file after 200 lines to bound cost.
fn recover_cwd_from_session(dir: &Path) -> Option<String> {
    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        let p = entry.path();
        if p.extension().and_then(|s| s.to_str()) != Some("jsonl") {
            continue;
        }
        if let Some(cwd) = read_cwd_from_jsonl(&p) {
            return Some(cwd);
        }
    }
    None
}

fn read_cwd_from_jsonl(file: &Path) -> Option<String> {
    use std::io::{BufRead, BufReader};
    let f = std::fs::File::open(file).ok()?;
    let reader = BufReader::new(f);
    for (i, line) in reader.lines().enumerate() {
        if i >= 200 {
            break;
        }
        let Ok(line) = line else { continue };
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) {
            if let Some(cwd) = v.get("cwd").and_then(|c| c.as_str()) {
                return Some(cwd.to_string());
            }
        }
    }
    None
}

fn scan_generic(
    root: &ScanRoot,
    known: &HashSet<String>,
    out: &mut BTreeMap<String, DetectedProject>,
) {
    let root_path = Path::new(&root.path);
    let entries = match std::fs::read_dir(root_path) {
        Ok(it) => it,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let p = entry.path();
        if !p.is_dir() {
            continue;
        }
        let signals = generic_signals(&p);
        if signals.is_empty() {
            continue;
        }

        let canonical = std::fs::canonicalize(&p)
            .map(|c| c.to_string_lossy().into_owned())
            .unwrap_or_else(|_| p.to_string_lossy().into_owned());

        out.entry(canonical.clone())
            .and_modify(|d| {
                for s in &signals {
                    if !d.signals.contains(s) {
                        d.signals.push(s.clone());
                    }
                }
            })
            .or_insert(DetectedProject {
                already_added: known.contains(&canonical),
                path: canonical,
                source_root: root.path.clone(),
                source_kind: root.kind,
                signals,
            });
    }
}

fn generic_signals(p: &Path) -> Vec<String> {
    let mut out = Vec::new();
    if p.join(".git").exists() {
        out.push("git".to_string());
    }
    if p.join(".claude").exists() {
        out.push("claude".to_string());
    }
    out
}

// We need a forward-declaration of `Project` for the `load_known_project_paths`
// helper. The full type lives in `models` but `commands::projects` (M2.5) is
// where the canonical CRUD lives — for now we just need to read the path field.
//
// (Resolved by adding `Project` to models in M2.5 — discover.rs imports it
// from there.)
#[allow(dead_code)]
const _PROJECT_TYPE_FORWARD_DECL: () = {
    fn _check(_: &Project) {}
};

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

    fn seed_cc_history(home: &Path, encoded: &str, cwd: &str) -> std::path::PathBuf {
        let dir = home.join(".claude/projects").join(encoded);
        std::fs::create_dir_all(&dir).unwrap();
        let session = dir.join("abc-123.jsonl");
        let line = serde_json::json!({"type": "x", "cwd": cwd}).to_string();
        std::fs::write(&session, line).unwrap();
        dir
    }

    #[test]
    #[serial(home_env)]
    fn cc_history_scan_recovers_cwd() {
        let g = setup_home();
        // Use AD_HOME for the project path so canonicalize succeeds.
        let real_proj = g.path().join("real-project");
        std::fs::create_dir_all(&real_proj).unwrap();

        seed_cc_history(
            g.path(),
            "-Users-yuanxuan-ai-workspace-real-project",
            real_proj.to_str().unwrap(),
        );

        let detected = scan_for_projects().unwrap();
        assert_eq!(detected.len(), 1);
        let canonical = std::fs::canonicalize(&real_proj)
            .unwrap()
            .to_string_lossy()
            .into_owned();
        assert_eq!(detected[0].path, canonical);
        assert!(detected[0].signals.contains(&"cc-history".to_string()));
        assert!(matches!(
            detected[0].source_kind,
            ScanRootKind::CcProjectsMeta
        ));
        assert!(!detected[0].already_added);
    }

    #[test]
    #[serial(home_env)]
    fn cc_history_skips_dirs_with_no_jsonl() {
        let g = setup_home();
        let dir = g.path().join(".claude/projects/-empty-encoded");
        std::fs::create_dir_all(&dir).unwrap();
        // No .jsonl file inside.
        let detected = scan_for_projects().unwrap();
        assert!(detected.is_empty(), "no candidate without cwd metadata");
    }

    #[test]
    #[serial(home_env)]
    fn cc_history_marks_missing_when_cwd_no_longer_exists() {
        let g = setup_home();
        seed_cc_history(
            g.path(),
            "-Users-yuanxuan-deleted",
            "/tmp/this-path-was-deleted-12345",
        );
        let detected = scan_for_projects().unwrap();
        assert_eq!(detected.len(), 1);
        assert!(detected[0].signals.contains(&"missing".to_string()));
    }

    #[test]
    #[serial(home_env)]
    fn generic_root_finds_subdirs_with_git() {
        let g = setup_home();
        let dev = g.path().join("dev");
        std::fs::create_dir_all(dev.join("foo/.git")).unwrap();
        std::fs::create_dir_all(dev.join("bar/.claude")).unwrap();
        std::fs::create_dir_all(dev.join("baz")).unwrap(); // no signals

        scan_roots::add_scan_root(dev.to_string_lossy().into_owned()).unwrap();
        // Disable the builtin so we only see results from our generic root.
        let builtin = scan_roots::list_scan_roots()
            .unwrap()
            .into_iter()
            .find(|r| r.builtin)
            .unwrap()
            .path;
        scan_roots::set_scan_root_enabled(builtin, false).unwrap();

        let detected = scan_for_projects().unwrap();
        let paths: Vec<&str> = detected.iter().map(|d| d.path.as_str()).collect();
        let has_foo = paths.iter().any(|p| p.ends_with("/foo"));
        let has_bar = paths.iter().any(|p| p.ends_with("/bar"));
        let has_baz = paths.iter().any(|p| p.ends_with("/baz"));
        assert!(has_foo, "foo should be detected (.git)");
        assert!(has_bar, "bar should be detected (.claude)");
        assert!(!has_baz, "baz has no signals → not detected");
    }

    #[test]
    #[serial(home_env)]
    fn already_added_is_set_when_in_projects_json() {
        let g = setup_home();
        let real_proj = g.path().join("known");
        std::fs::create_dir_all(&real_proj).unwrap();
        let canonical = std::fs::canonicalize(&real_proj)
            .unwrap()
            .to_string_lossy()
            .into_owned();

        // Seed projects.json
        let state_dir = g.path().join(".ad/state");
        std::fs::create_dir_all(&state_dir).unwrap();
        let state = serde_json::json!([{
            "path": canonical,
            "displayName": "Known",
            "addedAt": "2026-01-01T00:00:00Z",
            "currentProfileId": null,
            "lastApplied": null
        }]);
        std::fs::write(state_dir.join("projects.json"), state.to_string()).unwrap();

        seed_cc_history(g.path(), "-known", real_proj.to_str().unwrap());

        let detected = scan_for_projects().unwrap();
        assert_eq!(detected.len(), 1);
        assert!(detected[0].already_added);
    }
}
