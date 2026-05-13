//! `activate_profile`: backup + atomic write + history append + PID detect.
//!
//! All activations — UI-driven and tray-driven — funnel through `activate_profile`
//! which holds a process-wide mutex to avoid the menubar-rapid-click race
//! reported in code review.

use std::path::PathBuf;
use std::sync::Mutex;

use anyhow::Context;
use chrono::Utc;
use sysinfo::{ProcessRefreshKind, RefreshKind, System};

use crate::fs::atomic::write_atomic;
use crate::fs::paths::{backups_dir, claude_settings_path, ensure_dir, history_dir};
use crate::models::{ActivationLogEntry, ActivationResult, ClaudeProcess};

use super::profiles::{get_active_profile_id, get_profile, write_active_profile_id};
use super::CmdResult;

static ACTIVATE_LOCK: Mutex<()> = Mutex::new(());

#[tauri::command]
pub fn activate_profile(id: String) -> CmdResult<ActivationResult> {
    // Serialize all activations: prevents the active-pointer / settings.json
    // mismatch that two rapid tray clicks can produce.
    let _guard = ACTIVATE_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let profile = get_profile(id.clone())?;
    let from = get_active_profile_id().ok().flatten();

    let target = claude_settings_path()?;
    ensure_dir(target.parent().unwrap())?;

    // 1. Backup any prior settings.
    let backup_path = backup_current(&target)?;

    // 2. Atomic write of new settings.
    let bytes = serde_json::to_vec_pretty(&profile.settings)?;
    write_atomic(&target, &bytes)?;

    // 3. Update active pointer.
    write_active_profile_id(&profile.id)?;

    // 4. Append to history (per-file, crash-safe via write_atomic).
    write_history_entry(&ActivationLogEntry {
        ts: Utc::now(),
        from: from.clone(),
        to: profile.id.clone(),
        backup_path: backup_path.as_ref().map(|p| p.display().to_string()),
    })?;

    // 5. Detect running claude processes.
    let detected = detect_claude_processes_inner();

    Ok(ActivationResult {
        activated_id: profile.id,
        backup_path: backup_path.map(|p| p.display().to_string()),
        detected_pids: detected,
    })
}

#[tauri::command]
pub fn detect_claude_processes() -> CmdResult<Vec<ClaudeProcess>> {
    Ok(detect_claude_processes_inner())
}

fn detect_claude_processes_inner() -> Vec<ClaudeProcess> {
    let me = std::process::id();
    let mut sys = System::new_with_specifics(
        RefreshKind::nothing().with_processes(ProcessRefreshKind::nothing()),
    );
    sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);

    let mut out = Vec::new();
    for (pid, proc_) in sys.processes() {
        let pid_u32 = pid.as_u32();
        if pid_u32 == me {
            continue;
        }
        let name = proc_.name().to_string_lossy();
        if matches_claude(&name) {
            let cmd = proc_
                .cmd()
                .iter()
                .map(|s| s.to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join(" ");
            out.push(ClaudeProcess {
                pid: pid_u32,
                cmd: if cmd.is_empty() {
                    name.into_owned()
                } else {
                    cmd
                },
            });
        }
    }
    out
}

fn matches_claude(name: &str) -> bool {
    // sysinfo::Process::name() already returns the basename. We accept either
    // `claude` or `claude-code` (case-insensitive on macOS-installed bundles).
    let stem = name.trim_end_matches(".exe");
    stem.eq_ignore_ascii_case("claude") || stem.eq_ignore_ascii_case("claude-code")
}

fn backup_current(target: &std::path::Path) -> CmdResult<Option<PathBuf>> {
    if !target.exists() {
        return Ok(None);
    }
    let bytes = std::fs::read(target).with_context(|| format!("read {}", target.display()))?;
    if bytes.is_empty() {
        return Ok(None);
    }

    let backups = backups_dir()?;
    ensure_dir(&backups)?;

    // Filename-safe ISO 8601 with millisecond precision + uuid suffix avoids
    // collision under concurrent activations from the tray and main UI.
    let ts = Utc::now().format("%Y-%m-%dT%H-%M-%S%.3fZ").to_string();
    let suffix = &uuid::Uuid::new_v4().simple().to_string()[..6];
    let path = backups.join(format!("{ts}-{suffix}.json"));

    write_atomic(&path, &bytes)?;
    Ok(Some(path))
}

pub(crate) fn write_history_entry(entry: &ActivationLogEntry) -> CmdResult<()> {
    let dir = history_dir()?;
    ensure_dir(&dir)?;
    let ts = entry.ts.format("%Y-%m-%dT%H-%M-%S%.3fZ").to_string();
    let suffix = &uuid::Uuid::new_v4().simple().to_string()[..6];
    let path = dir.join(format!("{ts}-{suffix}.json"));
    let bytes = serde_json::to_vec(entry)?;
    write_atomic(&path, &bytes)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::profiles::save_profile;
    use crate::fs::paths::{claude_settings_path, history_dir};
    use crate::models::ProfileFile;
    use serial_test::serial;
    use tempfile::TempDir;

    fn setup() -> TempDir {
        let tmp = TempDir::new().unwrap();
        std::env::set_var("CC_SWITCH_HOME", tmp.path());
        tmp
    }

    #[test]
    #[serial(home_env)]
    fn activate_backs_up_and_writes_and_logs() {
        let _t = setup();
        save_profile(ProfileFile::sample()).unwrap();

        // Pre-existing settings.json
        let target = claude_settings_path().unwrap();
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(&target, br#"{"env":{"OLD":"1"}}"#).unwrap();

        let res = activate_profile("sample".into()).unwrap();
        assert_eq!(res.activated_id, "sample");

        // settings.json now contains new content
        let now: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&target).unwrap()).unwrap();
        assert!(now["env"].get("ANTHROPIC_BASE_URL").is_some());

        // backup created
        let backup = res.backup_path.expect("backup path");
        assert!(std::path::Path::new(&backup).exists());

        // history file appeared
        let count = std::fs::read_dir(history_dir().unwrap()).unwrap().count();
        assert_eq!(count, 1);
    }

    #[test]
    #[serial(home_env)]
    fn activate_with_no_prior_settings_skips_backup() {
        let _t = setup();
        save_profile(ProfileFile::sample()).unwrap();
        let res = activate_profile("sample".into()).unwrap();
        assert!(res.backup_path.is_none());
    }

    #[test]
    fn matches_claude_basename_only() {
        assert!(matches_claude("claude"));
        assert!(matches_claude("claude-code"));
        assert!(matches_claude("Claude"));
        assert!(!matches_claude("cc-switch"));
        assert!(!matches_claude("claude.app"));
    }
}
