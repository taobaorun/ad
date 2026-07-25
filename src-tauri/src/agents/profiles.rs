use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use crate::models::{ClaudeSettings, ProfileFile, ProfileLayers};

use super::{AgentAdapter, AgentId, ProfileId};

pub const AGENT_PROFILE_SCHEMA_VERSION: u32 = 1;
pub const CLAUDE_PROFILE_PAYLOAD_SCHEMA: &str = "ad.profile/claude-code.v2";
pub const CODEX_PROFILE_PAYLOAD_SCHEMA: &str = "ad.profile/codex.v1";

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentProfileKey {
    pub agent_id: AgentId,
    pub profile_id: ProfileId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProfileMetadata {
    pub display_name: String,
    pub description: Option<String>,
    pub color: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentProfile {
    pub schema_version: u32,
    pub key: AgentProfileKey,
    pub metadata: ProfileMetadata,
    pub payload_schema: String,
    pub payload: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ClaudeProfilePayload {
    pub layers: ProfileLayers,
    pub settings: ClaudeSettings,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CodexProfilePayload {
    pub config_toml: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProfileSettingsContent {
    pub media_type: String,
    pub content: Value,
}

pub trait ProfileSchema: Send + Sync {
    fn id(&self) -> &'static str;
    fn validate(&self, payload: &Value) -> Result<(), ProfileError>;
    fn settings_content(&self, payload: &Value) -> Result<ProfileSettingsContent, ProfileError>;
}

#[derive(Debug, Default)]
pub struct ClaudeProfileSchema;

#[derive(Debug, Default)]
pub struct CodexProfileSchema;

#[derive(Debug, Error)]
pub enum ProfileError {
    #[error("unsupported Agent profile schema: {0}")]
    UnsupportedSchema(String),
    #[error("invalid Agent profile: {0}")]
    Invalid(String),
    #[error("invalid Agent profile payload: {0}")]
    InvalidPayload(String),
}

impl ProfileSchema for ClaudeProfileSchema {
    fn id(&self) -> &'static str {
        CLAUDE_PROFILE_PAYLOAD_SCHEMA
    }

    fn validate(&self, payload: &Value) -> Result<(), ProfileError> {
        serde_json::from_value::<ClaudeProfilePayload>(payload.clone())
            .map(|_| ())
            .map_err(|error| ProfileError::InvalidPayload(error.to_string()))
    }

    fn settings_content(&self, payload: &Value) -> Result<ProfileSettingsContent, ProfileError> {
        let payload = serde_json::from_value::<ClaudeProfilePayload>(payload.clone())
            .map_err(|error| ProfileError::InvalidPayload(error.to_string()))?;
        let content = serde_json::to_value(payload.settings)
            .map_err(|error| ProfileError::InvalidPayload(error.to_string()))?;
        Ok(ProfileSettingsContent {
            media_type: "application/json".into(),
            content,
        })
    }
}

impl ProfileSchema for CodexProfileSchema {
    fn id(&self) -> &'static str {
        CODEX_PROFILE_PAYLOAD_SCHEMA
    }

    fn validate(&self, payload: &Value) -> Result<(), ProfileError> {
        let payload = serde_json::from_value::<CodexProfilePayload>(payload.clone())
            .map_err(|error| ProfileError::InvalidPayload(error.to_string()))?;
        payload
            .config_toml
            .parse::<toml::Value>()
            .map(|_| ())
            .map_err(|error| ProfileError::InvalidPayload(format!("invalid Codex TOML: {error}")))
    }

    fn settings_content(&self, payload: &Value) -> Result<ProfileSettingsContent, ProfileError> {
        let payload = serde_json::from_value::<CodexProfilePayload>(payload.clone())
            .map_err(|error| ProfileError::InvalidPayload(error.to_string()))?;
        payload
            .config_toml
            .parse::<toml::Value>()
            .map_err(|error| {
                ProfileError::InvalidPayload(format!("invalid Codex TOML: {error}"))
            })?;
        Ok(ProfileSettingsContent {
            media_type: "application/toml".into(),
            content: Value::String(payload.config_toml),
        })
    }
}

impl AgentProfile {
    pub fn from_legacy_claude(profile: ProfileFile) -> Result<Self, ProfileError> {
        if profile.agent_id != "claude-code" {
            return Err(ProfileError::Invalid(format!(
                "legacy ProfileFile belongs to {}, expected claude-code",
                profile.agent_id
            )));
        }
        let payload = serde_json::to_value(ClaudeProfilePayload {
            layers: profile.layers,
            settings: profile.settings,
        })
        .map_err(|error| ProfileError::InvalidPayload(error.to_string()))?;
        Ok(Self {
            schema_version: AGENT_PROFILE_SCHEMA_VERSION,
            key: AgentProfileKey {
                agent_id: AgentId::from("claude-code"),
                profile_id: ProfileId::from(profile.id),
            },
            metadata: ProfileMetadata {
                display_name: profile.display_name,
                description: profile.description,
                color: profile.color,
                created_at: profile.created_at,
                updated_at: profile.updated_at,
            },
            payload_schema: CLAUDE_PROFILE_PAYLOAD_SCHEMA.into(),
            payload,
        })
    }

    pub fn to_legacy_claude(&self) -> Result<ProfileFile, ProfileError> {
        if self.key.agent_id.as_str() != "claude-code"
            || self.payload_schema != CLAUDE_PROFILE_PAYLOAD_SCHEMA
        {
            return Err(ProfileError::Invalid(
                "only Claude Code profiles have a legacy representation".into(),
            ));
        }
        let payload = serde_json::from_value::<ClaudeProfilePayload>(self.payload.clone())
            .map_err(|error| ProfileError::InvalidPayload(error.to_string()))?;
        Ok(ProfileFile {
            id: self.key.profile_id.to_string(),
            display_name: self.metadata.display_name.clone(),
            description: self.metadata.description.clone(),
            agent_id: self.key.agent_id.to_string(),
            color: self.metadata.color.clone(),
            created_at: self.metadata.created_at,
            updated_at: self.metadata.updated_at,
            layers: payload.layers,
            settings: payload.settings,
        })
    }
}

pub fn validate_profile(
    adapter: &dyn AgentAdapter,
    profile: &AgentProfile,
) -> Result<(), ProfileError> {
    if profile.schema_version != AGENT_PROFILE_SCHEMA_VERSION {
        return Err(ProfileError::Invalid(format!(
            "unsupported envelope schema version: {}",
            profile.schema_version
        )));
    }
    if profile.key.agent_id != adapter.definition().id {
        return Err(ProfileError::Invalid(format!(
            "profile belongs to {}, expected {}",
            profile.key.agent_id,
            adapter.definition().id
        )));
    }
    if profile.metadata.display_name.trim().is_empty() {
        return Err(ProfileError::Invalid(
            "profile display name must not be empty".into(),
        ));
    }
    if !is_hex_color(&profile.metadata.color) {
        return Err(ProfileError::Invalid(
            "profile color must use #RRGGBB format".into(),
        ));
    }
    let schema = adapter
        .profile_schema()
        .ok_or_else(|| ProfileError::UnsupportedSchema(profile.payload_schema.clone()))?;
    if profile.payload_schema != schema.id() {
        return Err(ProfileError::UnsupportedSchema(
            profile.payload_schema.clone(),
        ));
    }
    schema.validate(&profile.payload)
}

pub fn profile_settings_content(
    adapter: &dyn AgentAdapter,
    profile: &AgentProfile,
) -> Result<ProfileSettingsContent, ProfileError> {
    validate_profile(adapter, profile)?;
    adapter
        .profile_schema()
        .ok_or_else(|| ProfileError::UnsupportedSchema(profile.payload_schema.clone()))?
        .settings_content(&profile.payload)
}

fn is_hex_color(color: &str) -> bool {
    color.len() == 7
        && color.starts_with('#')
        && color[1..].bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub fn decode_profile(bytes: &[u8]) -> Result<AgentProfile, ProfileError> {
    if let Ok(profile) = serde_json::from_slice::<AgentProfile>(bytes) {
        return Ok(profile);
    }
    let legacy = serde_json::from_slice::<ProfileFile>(bytes)
        .map_err(|error| ProfileError::Invalid(error.to_string()))?;
    AgentProfile::from_legacy_claude(legacy)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::builtin_registry;

    #[test]
    fn legacy_claude_profile_maps_to_an_envelope_without_changing_identity() {
        let legacy = ProfileFile::sample();
        let profile = AgentProfile::from_legacy_claude(legacy.clone()).unwrap();

        assert_eq!(profile.key.agent_id.as_str(), "claude-code");
        assert_eq!(profile.key.profile_id.as_str(), legacy.id);
        assert_eq!(profile.payload_schema, CLAUDE_PROFILE_PAYLOAD_SCHEMA);
        validate_profile(builtin_registry().adapter("claude-code").unwrap(), &profile).unwrap();
        assert_eq!(profile.to_legacy_claude().unwrap(), legacy);
    }

    #[test]
    fn codex_schema_accepts_toml_and_rejects_claude_payload() {
        let schema = CodexProfileSchema;
        schema
            .validate(&serde_json::json!({"configToml": "model = \"gpt-5.4\"\n"}))
            .unwrap();

        assert!(schema
            .validate(&serde_json::json!({"layers": {}, "settings": {"env": {}}}))
            .is_err());
        assert!(schema
            .validate(&serde_json::json!({"configToml": "[invalid"}))
            .is_err());
    }

    #[test]
    fn profile_identity_is_composite() {
        let claude = AgentProfileKey {
            agent_id: AgentId::from("claude-code"),
            profile_id: ProfileId::from("default"),
        };
        let codex = AgentProfileKey {
            agent_id: AgentId::from("codex"),
            profile_id: ProfileId::from("default"),
        };

        assert_ne!(claude, codex);
    }

    #[test]
    fn common_metadata_is_validated_before_adapter_payload() {
        let mut profile = AgentProfile::from_legacy_claude(ProfileFile::sample()).unwrap();
        profile.metadata.color = "purple".into();

        let error = validate_profile(builtin_registry().adapter("claude-code").unwrap(), &profile)
            .unwrap_err();

        assert!(error.to_string().contains("#RRGGBB"));
    }

    #[test]
    fn profile_payloads_produce_adapter_owned_settings_content() {
        let registry = builtin_registry();
        let claude_adapter = registry.adapter("claude-code").unwrap();
        let claude = AgentProfile::from_legacy_claude(ProfileFile::sample()).unwrap();
        let claude_content = profile_settings_content(claude_adapter, &claude).unwrap();
        assert_eq!(claude_content.media_type, "application/json");
        assert!(claude_content.content.is_object());

        let codex_adapter = registry.adapter("codex").unwrap();
        let mut codex = claude.clone();
        codex.key.agent_id = AgentId::from("codex");
        codex.payload_schema = CODEX_PROFILE_PAYLOAD_SCHEMA.into();
        codex.payload = serde_json::json!({"configToml": "model = \"gpt-5.4\"\n"});
        let codex_content = profile_settings_content(codex_adapter, &codex).unwrap();
        assert_eq!(codex_content.media_type, "application/toml");
        assert_eq!(codex_content.content, "model = \"gpt-5.4\"\n");
    }
}
