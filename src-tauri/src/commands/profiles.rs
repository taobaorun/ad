use std::path::PathBuf;

use anyhow::Context;
use chrono::Utc;

use crate::fs::atomic::write_atomic;
use crate::fs::paths::{active_pointer_path, ensure_dir, profiles_dir};
use crate::models::ProfileFile;

use super::{CmdResult, CommandError};

/// Validates a profile id. Strict allowlist: must start with alphanumeric and
/// contain only `[A-Za-z0-9._-]`, max 64 bytes. Rejects empty, NUL bytes,
/// control chars, Unicode bidi overrides, leading dot, and anything that could
/// resolve outside `profiles_dir()`.
pub(crate) fn validate_id(id: &str) -> Result<(), CommandError> {
    if id.is_empty() {
        return Err(CommandError::Generic("id is empty".into()));
    }
    if id.len() > 64 {
        return Err(CommandError::Generic(format!(
            "id too long ({} > 64 bytes)",
            id.len()
        )));
    }
    let mut chars = id.chars();
    let first = chars.next().expect("non-empty");
    if !first.is_ascii_alphanumeric() {
        return Err(CommandError::Generic(format!(
            "id must start with [A-Za-z0-9], got {first:?}"
        )));
    }
    for c in std::iter::once(first).chain(chars) {
        let ok = c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-';
        if !ok {
            return Err(CommandError::Generic(format!(
                "invalid character in id: {c:?}"
            )));
        }
    }
    Ok(())
}

fn profile_path(id: &str) -> CmdResult<PathBuf> {
    validate_id(id)?;
    Ok(profiles_dir()?.join(format!("{id}.json")))
}

fn validate_agent_id(agent_id: &str) -> CmdResult<()> {
    if agent_id != "claude-code" && agent_id != "codex" {
        return Err(CommandError::Generic(format!(
            "unknown built-in agent id: {agent_id}"
        )));
    }
    Ok(())
}

fn agent_profile_path(agent_id: &str, id: &str) -> CmdResult<PathBuf> {
    validate_agent_id(agent_id)?;
    validate_id(id)?;
    Ok(profiles_dir()?
        .join(agent_id)
        .join(format!("{id}.json")))
}

/// Returns true if a profile with the given id (case-insensitive) already
/// exists on disk. Used to refuse case-collisions on APFS-CI volumes.
fn id_collides_existing(id: &str) -> CmdResult<bool> {
    let dir = profiles_dir()?;
    if !dir.exists() {
        return Ok(false);
    }
    let lower = id.to_ascii_lowercase();
    for entry in std::fs::read_dir(&dir)? {
        let entry = entry?;
        let p = entry.path();
        if p.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        if let Some(stem) = p.file_stem().and_then(|s| s.to_str()) {
            if stem != id && stem.eq_ignore_ascii_case(&lower) {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

#[tauri::command]
pub fn list_profiles() -> CmdResult<Vec<ProfileFile>> {
    let dir = profiles_dir()?;
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&dir)? {
        let entry = entry?;
        let p = entry.path();
        if !p.is_file() {
            continue;
        }
        if p.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let bytes = std::fs::read(&p)?;
        match serde_json::from_slice::<ProfileFile>(&bytes) {
            Ok(profile) => out.push(profile),
            Err(e) => {
                tracing::warn!(path = %p.display(), error = %e, "skipping unreadable profile")
            }
        }
    }
    out.sort_by(|a, b| a.display_name.cmp(&b.display_name));
    Ok(out)
}

#[tauri::command]
pub fn get_profile(id: String) -> CmdResult<ProfileFile> {
    let path = profile_path(&id)?;
    let bytes = std::fs::read(&path).with_context(|| format!("read {}", path.display()))?;
    Ok(serde_json::from_slice::<ProfileFile>(&bytes)?)
}

#[tauri::command]
pub fn save_profile(profile: ProfileFile) -> CmdResult<ProfileFile> {
    let mut profile = profile;
    let path = profile_path(&profile.id)?;
    ensure_dir(path.parent().unwrap())?;

    // Case-collision check must run BEFORE path.exists() because APFS volumes
    // are case-insensitive: `path.exists()` for "homi.json" returns true when
    // "Homi.json" exists, masking the collision.
    if id_collides_existing(&profile.id)? {
        return Err(CommandError::Generic(format!(
            "id collides with an existing profile (case-insensitive): {}",
            profile.id
        )));
    }

    if path.exists() {
        let prev_bytes = std::fs::read(&path)?;
        if let Ok(prev) = serde_json::from_slice::<ProfileFile>(&prev_bytes) {
            if prev.updated_at > profile.updated_at {
                return Err(CommandError::Generic(format!(
                    "conflict: on-disk {} is newer ({} > {})",
                    profile.id, prev.updated_at, profile.updated_at
                )));
            }
            // preserve original createdAt
            profile.created_at = prev.created_at;
        }
    }

    profile.updated_at = Utc::now();
    let bytes = serde_json::to_vec_pretty(&profile)?;
    write_atomic(&path, &bytes)?;
    Ok(profile)
}

#[tauri::command]
pub fn delete_profile(id: String) -> CmdResult<()> {
    let path = profile_path(&id)?;
    if path.exists() {
        std::fs::remove_file(&path).with_context(|| format!("delete {}", path.display()))?;
    }
    Ok(())
}

#[tauri::command]
pub fn list_agent_profiles(agent_id: String) -> CmdResult<Vec<ProfileFile>> {
    validate_agent_id(&agent_id)?;
    if agent_id == "claude-code" {
        return Ok(list_profiles()?
            .into_iter()
            .filter(|profile| profile.agent_id == agent_id)
            .collect());
    }
    let dir = profiles_dir()?.join(&agent_id);
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() || path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let bytes = std::fs::read(&path)?;
        match serde_json::from_slice::<ProfileFile>(&bytes) {
            Ok(profile) if profile.agent_id == agent_id => out.push(profile),
            Ok(_) => tracing::warn!(path = %path.display(), "skipping profile with mismatched agent id"),
            Err(err) => tracing::warn!(path = %path.display(), error = %err, "skipping unreadable Agent profile"),
        }
    }
    out.sort_by(|a, b| a.display_name.cmp(&b.display_name));
    Ok(out)
}

#[tauri::command]
pub fn get_agent_profile(agent_id: String, id: String) -> CmdResult<ProfileFile> {
    validate_agent_id(&agent_id)?;
    if agent_id == "claude-code" {
        return get_profile(id);
    }
    let path = agent_profile_path(&agent_id, &id)?;
    let bytes = std::fs::read(&path).with_context(|| format!("read {}", path.display()))?;
    let profile: ProfileFile = serde_json::from_slice(&bytes)?;
    if profile.agent_id != agent_id {
        return Err(CommandError::Generic(format!(
            "profile agent mismatch: expected {agent_id}, found {}",
            profile.agent_id
        )));
    }
    Ok(profile)
}

#[tauri::command]
pub fn save_agent_profile(profile: ProfileFile) -> CmdResult<ProfileFile> {
    validate_agent_id(&profile.agent_id)?;
    if profile.agent_id == "claude-code" {
        return save_profile(profile);
    }
    let path = agent_profile_path(&profile.agent_id, &profile.id)?;
    ensure_dir(path.parent().unwrap())?;
    let mut profile = profile;
    if path.exists() {
        let prev_bytes = std::fs::read(&path)?;
        if let Ok(prev) = serde_json::from_slice::<ProfileFile>(&prev_bytes) {
            if prev.updated_at > profile.updated_at {
                return Err(CommandError::Generic(format!(
                    "conflict: on-disk {} is newer ({} > {})",
                    profile.id, prev.updated_at, profile.updated_at
                )));
            }
            profile.created_at = prev.created_at;
        }
    }
    profile.updated_at = Utc::now();
    write_atomic(&path, &serde_json::to_vec_pretty(&profile)?)?;
    Ok(profile)
}

#[tauri::command]
pub fn delete_agent_profile(agent_id: String, id: String) -> CmdResult<()> {
    validate_agent_id(&agent_id)?;
    if agent_id == "claude-code" {
        return delete_profile(id);
    }
    let path = agent_profile_path(&agent_id, &id)?;
    if path.exists() {
        std::fs::remove_file(&path).with_context(|| format!("delete {}", path.display()))?;
    }
    Ok(())
}

#[tauri::command]
pub fn get_active_profile_id() -> CmdResult<Option<String>> {
    let path = active_pointer_path()?;
    if !path.exists() {
        return Ok(None);
    }
    let s = std::fs::read_to_string(&path)?;
    let trimmed = s.trim();
    if trimmed.is_empty() {
        Ok(None)
    } else {
        Ok(Some(trimmed.to_string()))
    }
}

pub fn write_active_profile_id(id: &str) -> CmdResult<()> {
    let path = active_pointer_path()?;
    ensure_dir(path.parent().unwrap())?;
    write_atomic(&path, id.as_bytes())?;
    Ok(())
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
    fn list_save_get_roundtrip() {
        let _g = setup_home();
        let p = ProfileFile::sample();
        let saved = save_profile(p.clone()).unwrap();
        // Saving should bump updatedAt
        assert!(saved.updated_at >= p.updated_at);

        let listed = list_profiles().unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, "sample");

        let one = get_profile("sample".into()).unwrap();
        assert_eq!(one.display_name, "Sample");
    }

    #[test]
    #[serial(home_env)]
    fn agent_profiles_are_stored_under_agent_specific_directory() {
        let _g = setup_home();
        let mut profile = ProfileFile::sample();
        profile.agent_id = "codex".into();
        profile.id = "default".into();

        save_agent_profile(profile.clone()).unwrap();

        let listed = list_agent_profiles("codex".into()).unwrap();
        assert_eq!(listed.len(), 1);
        let loaded = get_agent_profile("codex".into(), "default".into()).unwrap();
        assert_eq!(loaded.agent_id, profile.agent_id);
        assert_eq!(loaded.id, profile.id);
        assert_eq!(loaded.settings, profile.settings);
        assert!(profiles_dir().unwrap().join("codex/default.json").exists());
        assert!(list_agent_profiles("claude-code".into()).unwrap().is_empty());
    }

    #[test]
    #[serial(home_env)]
    fn agent_profile_rejects_unknown_agent() {
        let _g = setup_home();
        let mut profile = ProfileFile::sample();
        profile.agent_id = "unknown".into();

        assert!(save_agent_profile(profile).is_err());
        assert!(list_agent_profiles("unknown".into()).is_err());
    }

    #[test]
    #[serial(home_env)]
    fn delete_removes_file() {
        let _g = setup_home();
        save_profile(ProfileFile::sample()).unwrap();
        delete_profile("sample".into()).unwrap();
        assert_eq!(list_profiles().unwrap().len(), 0);
    }

    #[test]
    #[serial(home_env)]
    fn invalid_id_rejected() {
        let _g = setup_home();
        for bad in [
            "../escape",
            "/abs",
            "..",
            ".hidden",
            "with space",
            "with/slash",
            "nul\0byte",
            "rtl\u{202E}",
            "ctrl\x01",
            "",
            &"x".repeat(65),
        ] {
            let mut p = ProfileFile::sample();
            p.id = bad.to_string();
            assert!(
                save_profile(p).is_err(),
                "expected rejection for id: {bad:?}"
            );
        }
    }

    #[test]
    #[serial(home_env)]
    fn layered_profile_roundtrip_persists_layers() {
        use crate::models::ProfileLayers;
        use std::collections::BTreeMap;

        let _g = setup_home();
        let mut env = BTreeMap::new();
        env.insert("ANTHROPIC_API_KEY".into(), "sk-test".into());

        let mut p = ProfileFile::sample();
        p.id = "layered".into();
        p.layers = ProfileLayers {
            shared: Some(serde_json::json!({
                "permissions": { "allow": ["fs:read"] }
            })),
            local: Some(serde_json::json!({ "model": "claude-opus-4-7" })),
            env,
        };

        save_profile(p.clone()).unwrap();
        let back = get_profile("layered".into()).unwrap();

        assert!(back.layers.shared.is_some());
        assert_eq!(
            back.layers.shared.unwrap()["permissions"]["allow"][0],
            "fs:read"
        );
        assert_eq!(back.layers.local.unwrap()["model"], "claude-opus-4-7");
        assert_eq!(
            back.layers.env.get("ANTHROPIC_API_KEY").map(String::as_str),
            Some("sk-test")
        );

        // Profiles persist under ~/.ad/profiles/ (the new location).
        let p_path = profiles_dir().unwrap().join("layered.json");
        assert!(p_path.exists());
    }

    #[test]
    #[serial(home_env)]
    fn case_collision_rejected() {
        let _g = setup_home();
        let mut a = ProfileFile::sample();
        a.id = "Homi".into();
        save_profile(a).unwrap();

        let mut b = ProfileFile::sample();
        b.id = "homi".into();
        let err = save_profile(b).unwrap_err();
        assert!(format!("{err}").contains("collides"), "got: {err}");
    }
}
