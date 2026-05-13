//! Canonical data model. Field naming uses `camelCase` at the FFI boundary so
//! TypeScript callers don't need to re-shape responses.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A profile saved at `~/.claude/profiles/<id>.json`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProfileFile {
    pub id: String,
    pub display_name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default = "default_color")]
    pub color: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub settings: ClaudeSettings,
}

/// Mirrors the relevant subset of `~/.claude/settings.json`.
///
/// `extra` captures any keys we don't model explicitly so we never lose data
/// when re-serializing.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeSettings {
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permissions: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hooks: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub theme: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ActivationResult {
    pub activated_id: String,
    pub backup_path: Option<String>,
    pub detected_pids: Vec<ClaudeProcess>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeProcess {
    pub pid: u32,
    pub cmd: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ActivationLogEntry {
    pub ts: DateTime<Utc>,
    pub from: Option<String>,
    pub to: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backup_path: Option<String>,
}

fn default_color() -> String {
    "#7C3AED".into()
}

impl ProfileFile {
    /// Reference fixture used by the Rust↔TS schema parity test.
    pub fn sample() -> Self {
        let now = chrono::Utc
            .with_ymd_and_hms(2026, 5, 13, 22, 55, 0)
            .unwrap();
        let mut env = BTreeMap::new();
        env.insert(
            "ANTHROPIC_BASE_URL".into(),
            "https://api.anthropic.com".into(),
        );
        env.insert("ANTHROPIC_MODEL".into(), "claude-opus-4-7".into());
        Self {
            id: "sample".into(),
            display_name: "Sample".into(),
            description: Some("Schema parity fixture".into()),
            color: default_color(),
            created_at: now,
            updated_at: now,
            settings: ClaudeSettings {
                env,
                permissions: None,
                hooks: None,
                model: Some("claude-opus-4-7".into()),
                theme: Some("dark".into()),
                extra: BTreeMap::new(),
            },
        }
    }
}

use chrono::TimeZone;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_sample() {
        let s = ProfileFile::sample();
        let json = serde_json::to_string_pretty(&s).unwrap();
        let back: ProfileFile = serde_json::from_str(&json).unwrap();
        assert_eq!(s, back);
    }

    #[test]
    fn camel_case_at_boundary() {
        let s = ProfileFile::sample();
        let json = serde_json::to_value(&s).unwrap();
        assert!(json.get("displayName").is_some());
        assert!(json.get("createdAt").is_some());
        assert!(json.get("updatedAt").is_some());
        // snake_case keys must NOT appear
        assert!(json.get("display_name").is_none());
    }

    #[test]
    fn unknown_settings_keys_preserved() {
        let raw = serde_json::json!({
            "env": {"X": "1"},
            "futureKey": {"nested": true}
        });
        let s: ClaudeSettings = serde_json::from_value(raw).unwrap();
        assert_eq!(s.env.get("X").map(String::as_str), Some("1"));
        assert!(s.extra.contains_key("futureKey"));

        let back = serde_json::to_value(&s).unwrap();
        assert!(back.get("futureKey").is_some());
    }

    #[test]
    fn write_sample_fixture() {
        let s = ProfileFile::sample();
        let path = std::path::Path::new("tests/fixtures/sample_profile.json");
        if let Some(p) = path.parent() {
            std::fs::create_dir_all(p).ok();
        }
        let json = serde_json::to_string_pretty(&s).unwrap() + "\n";
        std::fs::write(path, json).unwrap();
    }
}
