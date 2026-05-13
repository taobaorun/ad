use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::Context;

use crate::fs::atomic::write_atomic;
use crate::fs::paths::{backups_dir, claude_settings_path, ensure_dir, history_dir, history_path};
use crate::models::ActivationLogEntry;

use super::activate::write_history_entry;
use super::{CmdResult, CommandError};

#[tauri::command]
pub fn read_history(limit: Option<usize>) -> CmdResult<Vec<ActivationLogEntry>> {
    let mut out = Vec::new();
    let cap = limit.unwrap_or(usize::MAX);

    // Primary: per-file entries under cc-switch/history/.
    // Filenames begin with an ISO-8601 timestamp + uuid suffix, so lexical
    // sort = chronological sort. We sort filenames first and only read up to
    // `limit` of the newest, avoiding O(N) reads when N >> limit.
    let dir = history_dir()?;
    if dir.exists() {
        let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("json"))
            .collect();
        files.sort();
        files.reverse(); // newest first by filename

        for p in files.iter().take(cap) {
            match std::fs::read(p).and_then(|bytes| {
                serde_json::from_slice::<ActivationLogEntry>(&bytes)
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
            }) {
                Ok(e) => out.push(e),
                Err(e) => {
                    tracing::warn!(path = %p.display(), error = %e, "skipping malformed history entry")
                }
            }
        }
    }

    // Backward compat: read legacy line-delimited file if it exists. New
    // installs never write to it.
    if out.len() < cap {
        let legacy = history_path()?;
        if legacy.exists() {
            let text = std::fs::read_to_string(&legacy)?;
            for line in text.lines().rev() {
                if out.len() >= cap {
                    break;
                }
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                match serde_json::from_str::<ActivationLogEntry>(line) {
                    Ok(e) => out.push(e),
                    Err(e) => tracing::warn!(error = %e, "skipping malformed legacy history line"),
                }
            }
        }
    }

    // Final sort by timestamp — primary path is already newest-first by
    // filename, but legacy entries may interleave.
    out.sort_by_key(|e| e.ts);
    out.reverse();
    out.truncate(cap);
    Ok(out)
}

#[tauri::command]
pub fn restore_backup(backup_path: String) -> CmdResult<()> {
    let backup = PathBuf::from(&backup_path);
    let backups = backups_dir()?;
    // Ensure the backups dir exists so canonicalize can resolve it. If a user
    // somehow invokes restore_backup without ever having activated, this
    // creates an empty dir — a harmless no-op.
    ensure_dir(&backups)?;

    // Resolve the backups dir. We compare both case-folded paths because APFS
    // is case-insensitive by default and `starts_with` is byte-exact.
    let canonical_backups = backups
        .canonicalize()
        .with_context(|| format!("canonicalize backups dir {}", backups.display()))?;

    // Open the file FIRST: this binds the read to a specific inode and closes
    // the TOCTOU window between canonicalize and read. We canonicalize the
    // *parent* via the open file's metadata to enforce containment.
    let mut file =
        std::fs::File::open(&backup).with_context(|| format!("open {}", backup.display()))?;

    // Re-derive a canonical path of the open file via canonicalize on the
    // input path. Combined with the open fd above this leaves only a
    // microsecond TOCTOU window AND the actual data we read comes from the fd.
    let canonical_backup = backup
        .canonicalize()
        .with_context(|| format!("canonicalize {}", backup.display()))?;

    if !path_starts_with_case_insensitive(&canonical_backup, &canonical_backups) {
        return Err(CommandError::Generic(
            "refusing to restore from outside backups directory".into(),
        ));
    }

    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .with_context(|| format!("read {}", backup.display()))?;

    // Re-backup the current state, then write the restored content.
    let target = claude_settings_path()?;
    ensure_dir(target.parent().unwrap())?;

    if target.exists() {
        let ts = chrono::Utc::now()
            .format("%Y-%m-%dT%H-%M-%S%.3fZ")
            .to_string();
        let suffix = &uuid::Uuid::new_v4().simple().to_string()[..6];
        let pre = backups.join(format!("{ts}-{suffix}.json"));
        write_atomic(&pre, &std::fs::read(&target)?)?;
    }

    write_atomic(&target, &bytes)?;

    // Log the restore as a history entry. `to` is prefixed with `restore:`
    // so the UI can render it distinctly.
    write_history_entry(&ActivationLogEntry {
        ts: chrono::Utc::now(),
        from: super::profiles::get_active_profile_id().ok().flatten(),
        to: format!(
            "restore:{}",
            canonical_backup
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "<unknown>".into())
        ),
        backup_path: Some(canonical_backup.display().to_string()),
    })?;

    Ok(())
}

/// Case-insensitive `starts_with` for paths. macOS APFS volumes are
/// case-insensitive by default, so byte-exact comparison would reject a
/// validly-cased path that resolves to the same directory.
fn path_starts_with_case_insensitive(needle: &Path, haystack: &Path) -> bool {
    let n = needle.to_string_lossy().to_lowercase();
    let h = haystack.to_string_lossy().to_lowercase();
    // Ensure haystack ends with a separator so we don't accept `/foo/bar2`
    // as starting with `/foo/bar`.
    let h_with_sep = if h.ends_with('/') { h } else { format!("{h}/") };
    n.starts_with(&h_with_sep)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::activate::activate_profile;
    use crate::commands::profiles::save_profile;
    use crate::fs::paths::claude_settings_path;
    use crate::models::ProfileFile;
    use serial_test::serial;
    use tempfile::TempDir;

    #[test]
    #[serial(home_env)]
    fn rollback_restores_byte_identical() {
        let tmp = TempDir::new().unwrap();
        std::env::set_var("CC_SWITCH_HOME", tmp.path());

        // Profile A
        let mut a = ProfileFile::sample();
        a.id = "a".into();
        save_profile(a).unwrap();

        // Profile B
        let mut b = ProfileFile::sample();
        b.id = "b".into();
        b.settings.env.clear();
        b.settings.env.insert("FROM_B".into(), "1".into());
        save_profile(b).unwrap();

        // Activate A → activate B → restore the backup that B created
        activate_profile("a".into()).unwrap();
        let snapshot_a = std::fs::read(claude_settings_path().unwrap()).unwrap();

        let res_b = activate_profile("b".into()).unwrap();
        let backup_of_a = res_b.backup_path.unwrap();

        restore_backup(backup_of_a).unwrap();

        let now = std::fs::read(claude_settings_path().unwrap()).unwrap();
        assert_eq!(now, snapshot_a);
    }

    #[test]
    #[serial(home_env)]
    fn rejects_path_outside_backups_dir() {
        let tmp = TempDir::new().unwrap();
        std::env::set_var("CC_SWITCH_HOME", tmp.path());

        // Make a file outside backups_dir
        let evil = tmp.path().join("evil.json");
        std::fs::write(&evil, br#"{"env":{}}"#).unwrap();

        let err = restore_backup(evil.display().to_string()).unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("outside backups directory"),
            "expected containment error, got: {msg}"
        );
    }

    #[test]
    fn case_insensitive_starts_with() {
        assert!(path_starts_with_case_insensitive(
            Path::new("/Users/x/.claude/cc-switch/backups/file.json"),
            Path::new("/Users/X/.Claude/CC-Switch/Backups"),
        ));
        assert!(!path_starts_with_case_insensitive(
            Path::new("/Users/x/.claude/cc-switch/backups2/file.json"),
            Path::new("/Users/x/.claude/cc-switch/backups"),
        ));
    }
}
