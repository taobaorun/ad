//! Atomic file writes: write to a temp sibling, fsync, then rename. A crash
//! between `write` and `rename` leaves the original target untouched.
//!
//! **Caveat — parent directory fsync.** We do not fsync the parent directory
//! after rename. On macOS APFS this is fine: rename() metadata is journaled
//! and survives sudden power loss. On other filesystems (e.g. ext4 with
//! data=writeback) the rename could be lost. Since cc-switch is macOS-only
//! and APFS is the default, we accept this. If we ever ship to Linux, add
//! `File::open(parent)?.sync_all()?` after the rename.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use super::FsError;
use uuid::Uuid;

/// Write `bytes` to `target` via a temp sibling + fsync + rename.
///
/// The parent directory is created if missing. The temp file uses a random
/// suffix so concurrent writers don't collide.
pub fn write_atomic(target: &Path, bytes: &[u8]) -> Result<(), FsError> {
    let tmp = write_temp_only(target, bytes)?;
    std::fs::rename(&tmp, target).map_err(|e| FsError::io(target.display().to_string(), e))?;
    Ok(())
}

/// Internal: writes `bytes` to a uniquely-named sibling of `target`, fsyncs,
/// and returns the temp path. **Does not rename.** Used by `write_atomic` and
/// by the `crash_helper` integration test which `abort()`s after fsync to
/// prove the canonical path remains untouched.
pub fn write_temp_only(target: &Path, bytes: &[u8]) -> Result<PathBuf, FsError> {
    let parent = target
        .parent()
        .ok_or_else(|| FsError::InvalidPath(target.display().to_string()))?;

    if !parent.exists() {
        std::fs::create_dir_all(parent)
            .map_err(|e| FsError::io(parent.display().to_string(), e))?;
    }

    let tmp = temp_sibling(target);
    let mut f: File = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&tmp)
        .map_err(|e| FsError::io(tmp.display().to_string(), e))?;
    f.write_all(bytes)
        .map_err(|e| FsError::io(tmp.display().to_string(), e))?;
    f.sync_all()
        .map_err(|e| FsError::io(tmp.display().to_string(), e))?;
    Ok(tmp)
}

fn temp_sibling(target: &Path) -> PathBuf {
    let suffix = Uuid::new_v4().simple().to_string();
    let stem = target
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "cc-switch".into());
    target.with_file_name(format!(".{stem}.tmp.{suffix}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn happy_path() {
        let tmp = TempDir::new().unwrap();
        let target = tmp.path().join("a/b/file.json");
        write_atomic(&target, b"{\"hello\":1}").unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"{\"hello\":1}");
    }

    #[test]
    fn overwrite_existing() {
        let tmp = TempDir::new().unwrap();
        let target = tmp.path().join("file.json");
        std::fs::write(&target, b"old").unwrap();
        write_atomic(&target, b"new").unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"new");
    }

    #[test]
    fn concurrent_writes_to_distinct_paths_dont_collide() {
        let tmp = TempDir::new().unwrap();
        let mut handles = Vec::new();
        for i in 0..16 {
            let p = tmp.path().join(format!("f{i}.json"));
            handles.push(std::thread::spawn(move || {
                let payload = format!("{{\"i\":{i}}}");
                write_atomic(&p, payload.as_bytes()).unwrap();
            }));
        }
        for h in handles {
            h.join().unwrap();
        }
        for i in 0..16 {
            let p = tmp.path().join(format!("f{i}.json"));
            assert_eq!(
                std::fs::read_to_string(&p).unwrap(),
                format!("{{\"i\":{i}}}")
            );
        }
    }

    /// In-process pseudo-crash: stops after `write_temp_only` returns, before
    /// `rename` would happen. This is a smoke test only; the *real* invariant
    /// is exercised by `tests/atomic_crash.rs` which spawns a subprocess that
    /// `abort()`s.
    #[test]
    fn pseudo_crash_via_write_temp_only_preserves_original() {
        let tmp = TempDir::new().unwrap();
        let target = tmp.path().join("file.json");
        std::fs::write(&target, b"original").unwrap();

        // Production code path: write_temp_only is what `write_atomic` calls.
        let temp = write_temp_only(&target, b"half-written").unwrap();

        assert_eq!(std::fs::read(&target).unwrap(), b"original");
        assert_eq!(std::fs::read(&temp).unwrap(), b"half-written");
        assert!(temp.exists());
        std::fs::remove_file(&temp).unwrap();
    }
}
