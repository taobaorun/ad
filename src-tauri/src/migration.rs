//! First-run migrations.
//!
//! Two layers:
//! 1. **Data dir migration** (`migrate_data_dir_to_home`): moves AD's data from
//!    its v0.1 location under `~/.claude/ad/` and `~/.claude/profiles/` to
//!    `~/.ad/`. Runs first.
//! 2. **Legacy profile migration** (`migrate_legacy_profiles`): converts the
//!    even-older `{ displayName, env }` shape to the v1 `ProfileFile` shape.
//!
//! Both are **idempotent**: running them twice leaves disk state byte-identical
//! the second time.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result};
use chrono::Utc;
use serde::Deserialize;

use crate::fs::atomic::write_atomic;
use crate::fs::paths::{
    ad_home, claude_dir, ensure_dir, legacy_dir, legacy_v1_ad_dir, legacy_v1_profiles_dir,
    profiles_dir,
};
use crate::models::{ClaudeSettings, ProfileFile, ProfileLayers};

#[derive(Debug, Deserialize)]
struct LegacyProfile {
    #[serde(default)]
    #[serde(rename = "displayName")]
    display_name: Option<String>,
    #[serde(default)]
    env: Option<BTreeMap<String, String>>,
}

const MIGRATION_NOTE: &str = "\
# Migrated by ad

These files are the original profile JSONs from the legacy `{ displayName, env }` shape,
preserved here for safety. ad has migrated them to the new `ProfileFile` shape under
`~/.ad/profiles/`. Delete this directory at any time once you are sure you no longer need them.
";

/// Moves AD's data from the v0.1 layout (`~/.claude/ad/` + `~/.claude/profiles/`)
/// to the v0.2 layout (`~/.ad/`). Idempotent: returns `Ok(false)` if `~/.ad/`
/// already exists, otherwise moves and returns `Ok(true)`.
///
/// CC's own files (`~/.claude/settings.json`, `~/.claude/projects/`) are left
/// alone.
pub fn migrate_data_dir_to_home() -> Result<bool> {
    let new_home = ad_home().context("resolve ad_home")?;

    // Idempotent guard: if AD's new home already exists we assume migration
    // already happened (or the user set it up manually). Don't touch anything.
    if new_home.exists() {
        return Ok(false);
    }

    let old_ad = legacy_v1_ad_dir().context("resolve legacy v1 ad dir")?;
    let old_profiles = legacy_v1_profiles_dir().context("resolve legacy v1 profiles dir")?;

    // Nothing to migrate at all — fresh install.
    if !old_ad.exists() && !old_profiles.exists() {
        return Ok(false);
    }

    ensure_dir(&new_home).context("create ad_home")?;

    // Move ~/.claude/ad/* into ~/.ad/. We move children rather than the dir
    // itself so the destination structure is `~/.ad/{backups,history,active,...}`
    // and not `~/.ad/ad/{...}`.
    if old_ad.exists() {
        for entry in
            std::fs::read_dir(&old_ad).with_context(|| format!("read {}", old_ad.display()))?
        {
            let entry = entry?;
            let from = entry.path();
            let to = new_home.join(entry.file_name());
            std::fs::rename(&from, &to)
                .with_context(|| format!("mv {} -> {}", from.display(), to.display()))?;
        }
        // Remove the now-empty old dir. Ignore failure (e.g., if a hidden
        // file we missed is still there — leave it for the user).
        let _ = std::fs::remove_dir(&old_ad);
    }

    // Move ~/.claude/profiles -> ~/.ad/profiles (whole dir rename).
    if old_profiles.exists() {
        let new_profiles = new_home.join("profiles");
        std::fs::rename(&old_profiles, &new_profiles).with_context(|| {
            format!(
                "mv {} -> {}",
                old_profiles.display(),
                new_profiles.display()
            )
        })?;
    }

    // Drop a marker so a curious user opening ~/.claude/ later understands.
    let claude = claude_dir()?;
    if claude.exists() {
        let marker = claude.join("AD_MOVED_TO_HOME.txt");
        let body = format!(
            "AD migrated its data from ~/.claude/ to ~/.ad/ on {}.\n\
             Profiles, backups, history, and active state are now under ~/.ad/.\n\
             CC's own settings.json and projects/ remain in ~/.claude/.\n\
             You can delete this file at any time.\n",
            Utc::now().to_rfc3339()
        );
        // Best-effort: marker is informational, not load-bearing.
        let _ = write_atomic(&marker, body.as_bytes());
    }

    Ok(true)
}

/// Converts a v0.1 profile (flat `settings`) to a v0.2 profile (layered) by
/// **copying** the settings block into `layers.local`. The original `settings`
/// field is left intact so the legacy global-overwrite activation path keeps
/// working until it's sunset in M5. The original file is also backed up to
/// `<path>.v1.bak` for hard recovery.
///
/// `layers.local` is the safest target because it doesn't get committed to git
/// (CC reads it from `.claude/settings.local.json`).
///
/// Both fields stay in sync on disk until M5 sunsets the legacy path. M3's
/// layered editor will keep them in sync on save (TODO M3).
///
/// Idempotent: profiles with non-empty `layers` are skipped.
pub fn migrate_v1_profiles_to_layered() -> Result<usize> {
    let dir = profiles_dir().context("resolve profiles dir")?;
    if !dir.exists() {
        return Ok(0);
    }

    let mut migrated = 0usize;
    for entry in std::fs::read_dir(&dir).with_context(|| format!("read {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        // Only top-level .json files. .v1.bak (extension == "bak") is skipped.
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        match migrate_one_v1_to_layered(&path) {
            Ok(true) => migrated += 1,
            Ok(false) => {}
            Err(e) => tracing::warn!(
                path = %path.display(),
                error = %e,
                "v1 -> layered migration failed for one profile; skipping"
            ),
        }
    }
    Ok(migrated)
}

fn migrate_one_v1_to_layered(path: &Path) -> Result<bool> {
    let bytes = std::fs::read(path).with_context(|| format!("read {}", path.display()))?;
    let mut profile: ProfileFile =
        serde_json::from_slice(&bytes).with_context(|| format!("parse {}", path.display()))?;

    // Already migrated (any layer field set).
    if !profile.layers.is_empty() {
        return Ok(false);
    }
    // Nothing meaningful to migrate.
    if profile.settings.is_empty() {
        return Ok(false);
    }

    // Backup the original. Skip if .v1.bak already exists (prior partial run).
    let bak_name = format!(
        "{}.v1.bak",
        path.file_name()
            .ok_or_else(|| anyhow::anyhow!("no filename: {}", path.display()))?
            .to_string_lossy()
    );
    let bak = path.with_file_name(bak_name);
    if !bak.exists() {
        write_atomic(&bak, &bytes).with_context(|| format!("write backup {}", bak.display()))?;
    }

    // Settings → layers.local as raw JSON Value (preserves unknown fields).
    // Keep `settings` populated too: legacy activate_profile_inner still reads
    // it. Both fields will stay in sync until M5 sunsets the legacy path.
    let settings_value = serde_json::to_value(&profile.settings)?;
    profile.layers.local = Some(settings_value);
    profile.updated_at = Utc::now();

    let new_bytes = serde_json::to_vec_pretty(&profile)?;
    write_atomic(path, &new_bytes)?;

    Ok(true)
}

pub fn migrate_legacy_profiles() -> Result<usize> {
    let dir = profiles_dir().context("resolve profiles dir")?;
    if !dir.exists() {
        return Ok(0);
    }

    let mut migrated = 0usize;

    for entry in std::fs::read_dir(&dir).with_context(|| format!("read {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        // Per-file: a single corrupt legacy JSON must not block migration of
        // its siblings. Log and continue.
        match needs_migration(&path) {
            Ok(true) => match migrate_one(&path) {
                Ok(()) => migrated += 1,
                Err(e) => {
                    tracing::warn!(path = %path.display(), error = %e, "migration of one file failed; skipping")
                }
            },
            Ok(false) => {}
            Err(e) => {
                tracing::warn!(path = %path.display(), error = %e, "could not probe profile; skipping")
            }
        }
    }

    if migrated > 0 {
        let legacy = legacy_dir()?;
        ensure_dir(&legacy)?;
        let note = legacy.join("MIGRATION.md");
        if !note.exists() {
            write_atomic(&note, MIGRATION_NOTE.as_bytes())?;
        }
    }

    Ok(migrated)
}

fn needs_migration(path: &Path) -> Result<bool> {
    let bytes = std::fs::read(path).with_context(|| format!("read {}", path.display()))?;
    // Cheap shape probe: parse as Value, check whether `settings` is missing
    // and `env` is present.
    let v: serde_json::Value =
        serde_json::from_slice(&bytes).with_context(|| format!("parse {}", path.display()))?;
    let obj = match v.as_object() {
        Some(o) => o,
        None => return Ok(false),
    };
    Ok(obj.get("settings").is_none() && obj.get("env").is_some())
}

fn migrate_one(path: &Path) -> Result<()> {
    let bytes = std::fs::read(path)?;
    let legacy: LegacyProfile = serde_json::from_slice(&bytes)
        .with_context(|| format!("parse legacy {}", path.display()))?;

    let id = path
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| anyhow::anyhow!("invalid filename: {}", path.display()))?
        .to_string();

    let now = Utc::now();
    let settings = ClaudeSettings {
        env: legacy.env.unwrap_or_default(),
        ..Default::default()
    };
    let new_profile = ProfileFile {
        id: id.clone(),
        display_name: legacy.display_name.unwrap_or_else(|| id.clone()),
        description: None,
        color: "#7C3AED".into(),
        created_at: now,
        updated_at: now,
        layers: ProfileLayers::default(),
        settings,
    };

    // Save original under .legacy/<basename>.
    let legacy = legacy_dir()?;
    ensure_dir(&legacy)?;
    let legacy_target = legacy.join(path.file_name().unwrap());
    if !legacy_target.exists() {
        write_atomic(&legacy_target, &bytes)?;
    }

    // Overwrite the original location with the new shape.
    let new_bytes = serde_json::to_vec_pretty(&new_profile)?;
    write_atomic(path, &new_bytes)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use tempfile::TempDir;

    #[test]
    #[serial(home_env)]
    fn migrate_legacy_then_idempotent() {
        // We need the TempDir alive for the whole test, so don't use the helper above.
        let tmp = TempDir::new().unwrap();
        std::env::set_var("AD_HOME", tmp.path());

        let pdir = profiles_dir().unwrap();
        ensure_dir(&pdir).unwrap();

        let homi = pdir.join("homi.json");
        let legacy_json = serde_json::json!({
            "displayName": "Homi",
            "env": {
                "ANTHROPIC_BASE_URL": "https://example.com",
                "ANTHROPIC_MODEL": "GLM-5.1"
            }
        });
        std::fs::write(&homi, legacy_json.to_string()).unwrap();

        let migrated = migrate_legacy_profiles().unwrap();
        assert_eq!(migrated, 1);

        // Original copy preserved
        assert!(legacy_dir().unwrap().join("homi.json").exists());
        // New shape written
        let new: ProfileFile = serde_json::from_slice(&std::fs::read(&homi).unwrap()).unwrap();
        assert_eq!(new.display_name, "Homi");
        assert_eq!(
            new.settings.env.get("ANTHROPIC_MODEL").map(String::as_str),
            Some("GLM-5.1")
        );

        // Re-running migration must be a no-op
        let migrated2 = migrate_legacy_profiles().unwrap();
        assert_eq!(migrated2, 0);

        std::env::remove_var("AD_HOME");
    }

    /// Set up a v0.1 layout under `tmp` so we can exercise the data dir
    /// migration. Creates files inside both `~/.claude/ad/` and
    /// `~/.claude/profiles/` so we can verify they all move correctly.
    fn seed_v1_layout(tmp: &Path) {
        let claude = tmp.join(".claude");
        let old_ad = claude.join("ad");
        std::fs::create_dir_all(old_ad.join("backups")).unwrap();
        std::fs::create_dir_all(old_ad.join("history")).unwrap();
        std::fs::write(old_ad.join("active"), b"profile-x").unwrap();
        std::fs::write(old_ad.join("history.jsonl"), b"{\"old\":1}\n").unwrap();
        std::fs::write(
            old_ad.join("backups").join("2026-01-01.json"),
            b"{\"backed\":\"up\"}",
        )
        .unwrap();
        std::fs::write(
            old_ad.join("history").join("2026-01-01.json"),
            b"{\"hist\":1}",
        )
        .unwrap();

        let old_profiles = claude.join("profiles");
        std::fs::create_dir_all(&old_profiles).unwrap();
        std::fs::write(
            old_profiles.join("work.json"),
            b"{\"id\":\"work\",\"displayName\":\"Work\"}",
        )
        .unwrap();
    }

    #[test]
    #[serial(home_env)]
    fn data_dir_migration_moves_everything_then_idempotent() {
        let tmp = TempDir::new().unwrap();
        std::env::set_var("AD_HOME", tmp.path());

        seed_v1_layout(tmp.path());

        // Run the migration.
        let did_migrate = migrate_data_dir_to_home().unwrap();
        assert!(did_migrate, "first run should move data");

        // New layout: ~/.ad/{backups,history,active,history.jsonl,profiles}
        let new_home = ad_home().unwrap();
        assert!(new_home.exists());
        assert_eq!(
            std::fs::read(new_home.join("active")).unwrap(),
            b"profile-x"
        );
        assert_eq!(
            std::fs::read(new_home.join("history.jsonl")).unwrap(),
            b"{\"old\":1}\n"
        );
        assert!(new_home.join("backups/2026-01-01.json").exists());
        assert!(new_home.join("history/2026-01-01.json").exists());
        assert_eq!(
            std::fs::read(new_home.join("profiles/work.json")).unwrap(),
            b"{\"id\":\"work\",\"displayName\":\"Work\"}"
        );

        // Old layout: gone.
        assert!(!legacy_v1_ad_dir().unwrap().exists());
        assert!(!legacy_v1_profiles_dir().unwrap().exists());

        // Marker written.
        assert!(claude_dir().unwrap().join("AD_MOVED_TO_HOME.txt").exists());

        // Idempotent: second run is a no-op.
        let again = migrate_data_dir_to_home().unwrap();
        assert!(!again, "second run should be a no-op");

        std::env::remove_var("AD_HOME");
    }

    #[test]
    #[serial(home_env)]
    fn data_dir_migration_handles_only_ad_dir() {
        let tmp = TempDir::new().unwrap();
        std::env::set_var("AD_HOME", tmp.path());

        let old_ad = tmp.path().join(".claude/ad");
        std::fs::create_dir_all(old_ad.join("backups")).unwrap();
        std::fs::write(old_ad.join("active"), b"x").unwrap();

        let did = migrate_data_dir_to_home().unwrap();
        assert!(did);
        assert!(ad_home().unwrap().join("active").exists());
        // No profiles dir was created since none existed before.
        assert!(!ad_home().unwrap().join("profiles").exists());

        std::env::remove_var("AD_HOME");
    }

    #[test]
    #[serial(home_env)]
    fn data_dir_migration_handles_only_profiles() {
        let tmp = TempDir::new().unwrap();
        std::env::set_var("AD_HOME", tmp.path());

        let old_profiles = tmp.path().join(".claude/profiles");
        std::fs::create_dir_all(&old_profiles).unwrap();
        std::fs::write(old_profiles.join("a.json"), b"{}").unwrap();

        let did = migrate_data_dir_to_home().unwrap();
        assert!(did);
        assert!(ad_home().unwrap().join("profiles/a.json").exists());

        std::env::remove_var("AD_HOME");
    }

    #[test]
    #[serial(home_env)]
    fn data_dir_migration_skips_when_nothing_to_migrate() {
        let tmp = TempDir::new().unwrap();
        std::env::set_var("AD_HOME", tmp.path());

        let did = migrate_data_dir_to_home().unwrap();
        assert!(!did, "no v1 data → no-op");
        assert!(!ad_home().unwrap().exists(), "no ad_home should be created");

        std::env::remove_var("AD_HOME");
    }

    #[test]
    #[serial(home_env)]
    fn v1_to_layered_migrates_settings_into_layers_local() {
        let tmp = TempDir::new().unwrap();
        std::env::set_var("AD_HOME", tmp.path());

        let pdir = profiles_dir().unwrap();
        ensure_dir(&pdir).unwrap();

        // Seed a v1 profile (has settings, no layers).
        let v1 = serde_json::json!({
            "id": "work",
            "displayName": "Work",
            "color": "#7C3AED",
            "createdAt": "2026-01-01T00:00:00Z",
            "updatedAt": "2026-01-01T00:00:00Z",
            "settings": {
                "env": { "ANTHROPIC_API_KEY": "sk-test" },
                "model": "claude-opus-4-7"
            }
        });
        let path = pdir.join("work.json");
        std::fs::write(&path, v1.to_string()).unwrap();

        let migrated = migrate_v1_profiles_to_layered().unwrap();
        assert_eq!(migrated, 1);

        // .v1.bak preserves the original.
        let bak = pdir.join("work.json.v1.bak");
        assert!(bak.exists());
        let bak_bytes = std::fs::read(&bak).unwrap();
        let bak_v: serde_json::Value = serde_json::from_slice(&bak_bytes).unwrap();
        assert_eq!(bak_v["settings"]["model"], "claude-opus-4-7");

        // New file: layers.local has the settings content AND settings stays
        // populated (read by legacy activate path until M5 sunset).
        let new: ProfileFile = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert!(
            !new.settings.is_empty(),
            "v1 settings must stay populated for legacy activate compatibility"
        );
        assert_eq!(
            new.settings.model.as_deref(),
            Some("claude-opus-4-7"),
            "v1 settings.model must still be readable post-migration"
        );
        assert!(
            new.layers.local.is_some(),
            "layers.local should be populated"
        );
        let local = new.layers.local.as_ref().unwrap();
        assert_eq!(local["model"], "claude-opus-4-7");
        assert_eq!(local["env"]["ANTHROPIC_API_KEY"], "sk-test");

        // Idempotent: re-running is a no-op.
        let again = migrate_v1_profiles_to_layered().unwrap();
        assert_eq!(again, 0);

        std::env::remove_var("AD_HOME");
    }

    #[test]
    #[serial(home_env)]
    fn v1_to_layered_skips_already_layered_profiles() {
        let tmp = TempDir::new().unwrap();
        std::env::set_var("AD_HOME", tmp.path());

        let pdir = profiles_dir().unwrap();
        ensure_dir(&pdir).unwrap();

        // Profile with layers already populated (no need to migrate).
        let v2 = serde_json::json!({
            "id": "already",
            "displayName": "Already",
            "color": "#7C3AED",
            "createdAt": "2026-05-24T00:00:00Z",
            "updatedAt": "2026-05-24T00:00:00Z",
            "layers": {
                "local": { "model": "x" }
            }
        });
        std::fs::write(pdir.join("already.json"), v2.to_string()).unwrap();

        let migrated = migrate_v1_profiles_to_layered().unwrap();
        assert_eq!(migrated, 0);
        assert!(!pdir.join("already.json.v1.bak").exists());

        std::env::remove_var("AD_HOME");
    }

    #[test]
    #[serial(home_env)]
    fn v1_to_layered_skips_empty_settings() {
        let tmp = TempDir::new().unwrap();
        std::env::set_var("AD_HOME", tmp.path());

        let pdir = profiles_dir().unwrap();
        ensure_dir(&pdir).unwrap();

        // Profile with empty (default) settings — nothing to migrate.
        let empty = serde_json::json!({
            "id": "empty",
            "displayName": "Empty",
            "color": "#7C3AED",
            "createdAt": "2026-01-01T00:00:00Z",
            "updatedAt": "2026-01-01T00:00:00Z",
            "settings": { "env": {} }
        });
        std::fs::write(pdir.join("empty.json"), empty.to_string()).unwrap();

        let migrated = migrate_v1_profiles_to_layered().unwrap();
        assert_eq!(migrated, 0);

        std::env::remove_var("AD_HOME");
    }

    #[test]
    #[serial(home_env)]
    fn data_dir_migration_skips_when_new_home_already_exists() {
        let tmp = TempDir::new().unwrap();
        std::env::set_var("AD_HOME", tmp.path());

        seed_v1_layout(tmp.path());
        // Pre-create ~/.ad/ to simulate a user who set things up manually.
        std::fs::create_dir_all(ad_home().unwrap()).unwrap();
        std::fs::write(ad_home().unwrap().join("manual.txt"), b"keep me").unwrap();

        let did = migrate_data_dir_to_home().unwrap();
        assert!(!did, "ad_home exists → skip");

        // Old data still in place (untouched).
        assert!(legacy_v1_ad_dir().unwrap().join("active").exists());
        assert!(legacy_v1_profiles_dir().unwrap().join("work.json").exists());
        // Manual file preserved.
        assert_eq!(
            std::fs::read(ad_home().unwrap().join("manual.txt")).unwrap(),
            b"keep me"
        );

        std::env::remove_var("AD_HOME");
    }
}
