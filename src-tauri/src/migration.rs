//! First-run migration of legacy profile files.
//!
//! Legacy shape: `{ "displayName": "...", "env": { ... } }` — no `settings`.
//! New shape: `{ "id", "displayName", "createdAt", "updatedAt", "settings": { "env": ... } }`.
//!
//! Migration is **idempotent**: running it twice leaves disk state byte-identical
//! the second time. Originals are copied to `~/.claude/profiles/.legacy/` and a
//! `MIGRATION.md` note is written there once explaining what happened.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result};
use chrono::Utc;
use serde::Deserialize;

use crate::fs::atomic::write_atomic;
use crate::fs::paths::{ensure_dir, legacy_dir, profiles_dir};
use crate::models::{ClaudeSettings, ProfileFile};

#[derive(Debug, Deserialize)]
struct LegacyProfile {
    #[serde(default)]
    #[serde(rename = "displayName")]
    display_name: Option<String>,
    #[serde(default)]
    env: Option<BTreeMap<String, String>>,
}

const MIGRATION_NOTE: &str = "\
# Migrated by cc-switch

These files are the original profile JSONs from the legacy `{ displayName, env }` shape,
preserved here for safety. cc-switch has migrated them to the new `ProfileFile` shape under
`~/.claude/profiles/`. Delete this directory at any time once you are sure you no longer need them.
";

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
        if needs_migration(&path)? {
            migrate_one(&path)?;
            migrated += 1;
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
        std::env::set_var("CC_SWITCH_HOME", tmp.path());

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

        std::env::remove_var("CC_SWITCH_HOME");
    }
}
