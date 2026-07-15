use std::collections::BTreeMap;

use anyhow::Context;
use chrono::Utc;

use crate::agents::{
    builtin_registry, decode_profile, validate_profile, AgentAdapter, AgentProfile, ProfileError,
};
use crate::fs::atomic::write_atomic;
use crate::fs::paths::{ensure_dir, profiles_dir};

use super::profiles::{
    agent_profile_path, id_collides_in_dir, profile_path, validate_agent_id, validate_id,
};
use super::{CmdResult, CommandError};

#[tauri::command]
pub fn list_profile_envelopes(agent_id: String) -> CmdResult<Vec<AgentProfile>> {
    validate_agent_id(&agent_id)?;
    let registry = builtin_registry();
    let adapter = registry
        .adapter(&agent_id)
        .ok_or_else(|| CommandError::Generic(format!("unknown built-in agent id: {agent_id}")))?;
    let mut profiles = BTreeMap::new();

    if agent_id == "claude-code" {
        collect_profile_envelopes(&profiles_dir()?, adapter, &agent_id, &mut profiles)?;
    }
    collect_profile_envelopes(
        &profiles_dir()?.join(&agent_id),
        adapter,
        &agent_id,
        &mut profiles,
    )?;

    let mut profiles = profiles.into_values().collect::<Vec<_>>();
    profiles.sort_by(|left, right| left.metadata.display_name.cmp(&right.metadata.display_name));
    Ok(profiles)
}

#[tauri::command]
pub fn get_profile_envelope(agent_id: String, id: String) -> CmdResult<AgentProfile> {
    validate_agent_id(&agent_id)?;
    validate_id(&id)?;
    let canonical_path = agent_profile_path(&agent_id, &id)?;
    let path = if canonical_path.is_file() {
        canonical_path
    } else if agent_id == "claude-code" {
        profile_path(&id)?
    } else {
        canonical_path
    };
    let bytes = std::fs::read(&path).with_context(|| format!("read {}", path.display()))?;
    decode_and_validate_profile(&agent_id, &id, &bytes)
}

#[tauri::command]
pub fn save_profile_envelope(profile: AgentProfile) -> CmdResult<AgentProfile> {
    let agent_id = profile.key.agent_id.to_string();
    let profile_id = profile.key.profile_id.to_string();
    validate_agent_id(&agent_id)?;
    validate_id(&profile_id)?;
    let registry = builtin_registry();
    let adapter = registry
        .adapter(&agent_id)
        .ok_or_else(|| CommandError::Generic(format!("unknown built-in agent id: {agent_id}")))?;
    validate_profile(adapter, &profile)
        .map_err(|error| CommandError::Generic(error.to_string()))?;

    let dir = profiles_dir()?.join(&agent_id);
    if id_collides_in_dir(&dir, &profile_id)? {
        return Err(CommandError::Generic(format!(
            "id collides with an existing profile (case-insensitive): {profile_id}"
        )));
    }
    if agent_id == "claude-code" && id_collides_in_dir(&profiles_dir()?, &profile_id)? {
        return Err(CommandError::Generic(format!(
            "id collides with an existing legacy profile (case-insensitive): {profile_id}"
        )));
    }
    let path = agent_profile_path(&agent_id, &profile_id)?;
    ensure_dir(path.parent().unwrap())?;
    let mut profile = profile;
    let previous_path = if path.is_file() {
        Some(path.clone())
    } else if agent_id == "claude-code" {
        let legacy_path = profile_path(&profile_id)?;
        legacy_path.is_file().then_some(legacy_path)
    } else {
        None
    };
    if let Some(previous_path) = previous_path {
        let previous =
            decode_and_validate_profile(&agent_id, &profile_id, &std::fs::read(previous_path)?)?;
        if previous.metadata.updated_at > profile.metadata.updated_at {
            return Err(CommandError::Generic(format!(
                "conflict: on-disk {} is newer ({} > {})",
                profile.key.profile_id, previous.metadata.updated_at, profile.metadata.updated_at
            )));
        }
        profile.metadata.created_at = previous.metadata.created_at;
    }
    profile.metadata.updated_at = Utc::now();
    write_atomic(&path, &serde_json::to_vec_pretty(&profile)?)?;
    Ok(profile)
}

#[tauri::command]
pub fn delete_profile_envelope(agent_id: String, id: String) -> CmdResult<()> {
    let path = agent_profile_path(&agent_id, &id)?;
    if path.is_file() {
        std::fs::remove_file(&path).with_context(|| format!("delete {}", path.display()))?;
    }
    if agent_id == "claude-code" {
        let legacy_path = profile_path(&id)?;
        if legacy_path.is_file() {
            std::fs::remove_file(&legacy_path)
                .with_context(|| format!("delete {}", legacy_path.display()))?;
        }
    }
    Ok(())
}

fn collect_profile_envelopes(
    dir: &std::path::Path,
    adapter: &dyn AgentAdapter,
    expected_agent_id: &str,
    profiles: &mut BTreeMap<String, AgentProfile>,
) -> CmdResult<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if !path.is_file() || path.extension().and_then(|suffix| suffix.to_str()) != Some("json") {
            continue;
        }
        let bytes = std::fs::read(&path)?;
        match decode_profile(&bytes).and_then(|profile| {
            if profile.key.agent_id.as_str() != expected_agent_id {
                return Err(ProfileError::Invalid(format!(
                    "profile belongs to {}, expected {expected_agent_id}",
                    profile.key.agent_id
                )));
            }
            let file_id = path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or_default();
            validate_id(file_id).map_err(|error| ProfileError::Invalid(error.to_string()))?;
            if profile.key.profile_id.as_str() != file_id {
                return Err(ProfileError::Invalid(format!(
                    "profile id {} does not match file name {file_id}",
                    profile.key.profile_id
                )));
            }
            validate_profile(adapter, &profile)?;
            Ok(profile)
        }) {
            Ok(profile) => {
                profiles.insert(profile.key.profile_id.to_string(), profile);
            }
            Err(error) => {
                tracing::warn!(path = %path.display(), error = %error, "skipping unreadable Agent profile")
            }
        }
    }
    Ok(())
}

fn decode_and_validate_profile(
    agent_id: &str,
    profile_id: &str,
    bytes: &[u8],
) -> CmdResult<AgentProfile> {
    let profile =
        decode_profile(bytes).map_err(|error| CommandError::Generic(error.to_string()))?;
    if profile.key.agent_id.as_str() != agent_id {
        return Err(CommandError::Generic(format!(
            "profile agent mismatch: expected {agent_id}, found {}",
            profile.key.agent_id
        )));
    }
    if profile.key.profile_id.as_str() != profile_id {
        return Err(CommandError::Generic(format!(
            "profile id mismatch: expected {profile_id}, found {}",
            profile.key.profile_id
        )));
    }
    let registry = builtin_registry();
    let adapter = registry
        .adapter(agent_id)
        .ok_or_else(|| CommandError::Generic(format!("unknown built-in agent id: {agent_id}")))?;
    validate_profile(adapter, &profile)
        .map_err(|error| CommandError::Generic(error.to_string()))?;
    Ok(profile)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::{
        AgentId, AgentProfileKey, CodexProfilePayload, ProfileId, ProfileMetadata,
        AGENT_PROFILE_SCHEMA_VERSION, CLAUDE_PROFILE_PAYLOAD_SCHEMA, CODEX_PROFILE_PAYLOAD_SCHEMA,
    };
    use crate::models::ProfileFile;
    use serial_test::serial;

    fn setup_home() -> tempfile::TempDir {
        let temp = tempfile::tempdir().unwrap();
        std::env::set_var("AD_HOME", temp.path());
        temp
    }

    fn codex_profile(id: &str, config_toml: &str) -> AgentProfile {
        let now = Utc::now();
        AgentProfile {
            schema_version: AGENT_PROFILE_SCHEMA_VERSION,
            key: AgentProfileKey {
                agent_id: AgentId::from("codex"),
                profile_id: ProfileId::from(id),
            },
            metadata: ProfileMetadata {
                display_name: "Codex Default".into(),
                description: None,
                color: "#7C3AED".into(),
                created_at: now,
                updated_at: now,
            },
            payload_schema: CODEX_PROFILE_PAYLOAD_SCHEMA.into(),
            payload: serde_json::to_value(CodexProfilePayload {
                config_toml: config_toml.into(),
            })
            .unwrap(),
        }
    }

    #[test]
    #[serial(home_env)]
    fn legacy_claude_profile_is_read_without_rewriting_source() {
        let _guard = setup_home();
        let original = include_bytes!("../../tests/fixtures/sample_profile.json").to_vec();
        let legacy: ProfileFile = serde_json::from_slice(&original).unwrap();
        let path = profile_path(&legacy.id).unwrap();
        ensure_dir(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &original).unwrap();

        let profiles = list_profile_envelopes("claude-code".into()).unwrap();

        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].key.profile_id.as_str(), legacy.id);
        assert_eq!(profiles[0].payload_schema, CLAUDE_PROFILE_PAYLOAD_SCHEMA);
        assert_eq!(std::fs::read(path).unwrap(), original);
    }

    #[test]
    #[serial(home_env)]
    fn envelopes_use_composite_identity_and_adapter_payloads() {
        let _guard = setup_home();
        let mut legacy = ProfileFile::sample();
        legacy.id = "default".into();
        let claude = AgentProfile::from_legacy_claude(legacy).unwrap();

        save_profile_envelope(claude).unwrap();
        save_profile_envelope(codex_profile("default", "model = \"gpt-5.4\"\n")).unwrap();

        let claude = get_profile_envelope("claude-code".into(), "default".into()).unwrap();
        let codex = get_profile_envelope("codex".into(), "default".into()).unwrap();
        assert_eq!(claude.key.profile_id, codex.key.profile_id);
        assert_ne!(claude.key.agent_id, codex.key.agent_id);
        assert_eq!(codex.payload["configToml"], "model = \"gpt-5.4\"\n");
        assert!(codex.payload.get("layers").is_none());
        assert!(profiles_dir()
            .unwrap()
            .join("claude-code/default.json")
            .is_file());
        assert!(profiles_dir().unwrap().join("codex/default.json").is_file());
    }

    #[test]
    #[serial(home_env)]
    fn codex_profile_rejects_invalid_toml_payload() {
        let _guard = setup_home();

        let error = save_profile_envelope(codex_profile("invalid", "[invalid")).unwrap_err();

        assert!(format!("{error}").contains("invalid Codex TOML"));
    }

    #[test]
    #[serial(home_env)]
    fn envelope_file_name_must_match_composite_profile_identity() {
        let _guard = setup_home();
        let profile = codex_profile("other", "model = \"gpt-5.4\"\n");
        let path = agent_profile_path("codex", "default").unwrap();
        ensure_dir(path.parent().unwrap()).unwrap();
        std::fs::write(path, serde_json::to_vec(&profile).unwrap()).unwrap();

        let error = get_profile_envelope("codex".into(), "default".into()).unwrap_err();

        assert!(format!("{error}").contains("profile id mismatch"));
        assert!(list_profile_envelopes("codex".into()).unwrap().is_empty());
    }

    #[test]
    #[serial(home_env)]
    fn envelope_list_skips_invalid_profile_identity() {
        let _guard = setup_home();
        let profile = codex_profile(".hidden", "model = \"gpt-5.4\"\n");
        let dir = profiles_dir().unwrap().join("codex");
        ensure_dir(&dir).unwrap();
        std::fs::write(
            dir.join(".hidden.json"),
            serde_json::to_vec(&profile).unwrap(),
        )
        .unwrap();

        assert!(list_profile_envelopes("codex".into()).unwrap().is_empty());
    }

    #[test]
    #[serial(home_env)]
    fn legacy_claude_conflict_is_checked_before_canonical_save() {
        let _guard = setup_home();
        let mut legacy = ProfileFile::sample();
        legacy.updated_at = Utc::now() + chrono::Duration::minutes(1);
        let path = profile_path(&legacy.id).unwrap();
        ensure_dir(path.parent().unwrap()).unwrap();
        std::fs::write(&path, serde_json::to_vec(&legacy).unwrap()).unwrap();
        let mut envelope = AgentProfile::from_legacy_claude(legacy).unwrap();
        envelope.metadata.updated_at -= chrono::Duration::minutes(2);

        let error = save_profile_envelope(envelope).unwrap_err();

        assert!(format!("{error}").contains("on-disk"));
        assert!(!profiles_dir()
            .unwrap()
            .join("claude-code/sample.json")
            .exists());
    }

    #[test]
    #[serial(home_env)]
    fn deleting_claude_envelope_removes_canonical_and_legacy_representations() {
        let _guard = setup_home();
        let legacy = ProfileFile::sample();
        let legacy_path = profile_path(&legacy.id).unwrap();
        ensure_dir(legacy_path.parent().unwrap()).unwrap();
        std::fs::write(&legacy_path, serde_json::to_vec(&legacy).unwrap()).unwrap();
        let envelope = AgentProfile::from_legacy_claude(legacy).unwrap();
        save_profile_envelope(envelope).unwrap();
        let canonical_path = agent_profile_path("claude-code", "sample").unwrap();

        delete_profile_envelope("claude-code".into(), "sample".into()).unwrap();

        assert!(!legacy_path.exists());
        assert!(!canonical_path.exists());
        assert!(list_profile_envelopes("claude-code".into())
            .unwrap()
            .is_empty());
    }

    #[test]
    #[serial(home_env)]
    fn canonical_claude_save_rejects_legacy_case_collision() {
        let _guard = setup_home();
        let mut legacy = ProfileFile::sample();
        legacy.id = "Default".into();
        let legacy_path = profile_path(&legacy.id).unwrap();
        ensure_dir(legacy_path.parent().unwrap()).unwrap();
        std::fs::write(legacy_path, serde_json::to_vec(&legacy).unwrap()).unwrap();
        legacy.id = "default".into();
        let envelope = AgentProfile::from_legacy_claude(legacy).unwrap();

        let error = save_profile_envelope(envelope).unwrap_err();

        assert!(format!("{error}").contains("legacy profile"));
    }
}
