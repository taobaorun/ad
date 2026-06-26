//! Canonical data model. Field naming uses `camelCase` at the FFI boundary so
//! TypeScript callers don't need to re-shape responses.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A profile saved at `~/.ad/profiles/<id>.json`.
///
/// v0.2 introduces the `layers` field for the layered profile recipe model
/// (shared / local / env). The legacy `settings` field is retained for
/// backward-compat reads and for the legacy global-overwrite activation path.
/// Newly-edited profiles populate `layers`; the migration in
/// `migrate_v1_profiles_to_layered` copies old `settings` content into
/// `layers.local`.
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

    /// Layered settings recipe. New in v0.2.
    #[serde(default)]
    pub layers: ProfileLayers,

    /// Legacy v0.1 flat settings block. Still read for backward-compat and used
    /// by the legacy global-overwrite activation path. New flow uses `layers`.
    #[serde(default)]
    pub settings: ClaudeSettings,
}

/// Layered profile recipe (v0.2).
///
/// - `shared` is applied to `<project>/.claude/settings.json` (committed to git).
/// - `local` is applied to `<project>/.claude/settings.local.json` (gitignored).
/// - `env` is presented to the user as `export KEY=VALUE` snippets — never written to a file.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct ProfileLayers {
    /// Goes to `<project>/.claude/settings.json`. None means the profile does
    /// not contribute to the shared layer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shared: Option<Value>,

    /// Goes to `<project>/.claude/settings.local.json`. None means the profile
    /// does not contribute to the local layer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local: Option<Value>,

    /// Environment variables presented as `export` snippets. Empty by default.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
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

/// A project AD tracks. Stored as an entry in `~/.ad/state/projects.json`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    /// Canonical absolute path; primary key.
    pub path: String,
    /// Default = path basename; mutable via rename.
    pub display_name: String,
    pub added_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_profile_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_applied: Option<LastApplied>,
    /// User-pinned projects sort to the top of the sidebar. Defaults to false
    /// so pre-existing registry entries deserialize without a `pinned` key.
    #[serde(default)]
    pub pinned: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LastApplied {
    pub profile_id: String,
    pub timestamp: DateTime<Utc>,
    /// Which layers were written this round, e.g. `["local", "env"]`.
    pub layers: Vec<String>,
    /// Backup file paths produced (under `~/.ad/backups/`).
    #[serde(default)]
    pub backup_paths: Vec<String>,
    /// Number of conflicts the user resolved during this apply (audit only).
    #[serde(default)]
    pub conflicts_resolved: usize,
}

/// One root directory to scan for project candidates. Stored at
/// `~/.ad/state/scan_roots.json`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ScanRoot {
    /// Absolute path to the root.
    pub path: String,
    pub kind: ScanRootKind,
    /// Builtin roots (e.g. `~/.claude/projects`) cannot be removed; only
    /// toggled via `enabled`.
    #[serde(default)]
    pub builtin: bool,
    /// Whether this root is currently scanned. Allows users to keep a builtin
    /// root in the list but skip it in detection.
    #[serde(default = "default_true")]
    pub enabled: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ScanRootKind {
    /// `~/.claude/projects/<encoded-path>/` — CC's per-project metadata.
    /// AD reverse-decodes the encoded path to recover the original project
    /// directory.
    CcProjectsMeta,
    /// Arbitrary directory. AD walks one level deep, treating any subdir
    /// containing `.git/` or `.claude/` as a project candidate.
    Generic,
}

fn default_true() -> bool {
    true
}

/// Live filesystem snapshot of a project (returned by `get_project_status`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectStatus {
    pub exists: bool,
    pub is_git_repo: bool,
    pub git_dirty: bool,
    pub claude_dir_exists: bool,
    pub has_settings_json: bool,
    pub has_settings_local_json: bool,
    /// `Some(true)` if `.gitignore` excludes `.claude/settings.local.json`,
    /// `Some(false)` if it doesn't, `None` if not a git repo or no `.gitignore`.
    pub gitignore_excludes_settings_local: Option<bool>,
}

/// A project candidate surfaced by `commands::discover::scan_for_projects`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DetectedProject {
    /// Canonical absolute path to the candidate project.
    pub path: String,
    /// The scan root this candidate came from (matches a `ScanRoot.path`).
    pub source_root: String,
    /// The kind of root that produced it (passes through to UI for labeling).
    pub source_kind: ScanRootKind,
    /// What signal(s) marked this as a project. e.g. `["cc-history"]`,
    /// `["git", "claude"]`. Lowercased lowercase ASCII tags.
    pub signals: Vec<String>,
    /// Whether AD already tracks this path in its projects.json.
    pub already_added: bool,
}

fn default_color() -> String {
    "#7C3AED".into()
}

// ---------------------------------------------------------------------------
// Skill management (v0.5)
// ---------------------------------------------------------------------------

/// A configured skill source — git repo or local directory.
/// Stored as JSON array in `~/.ad/state/skill_sources.json`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SkillSource {
    pub id: String,
    pub source_type: SkillSourceType,
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subdirectory: Option<String>,
    #[serde(default)]
    pub auto_update: bool,
    pub added_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SkillSourceType {
    Git,
    Local,
}

/// Per-project skill configuration.
/// Stored at `~/.ad/state/project_skills/<slug>.json`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSkillConfig {
    pub project_path: String,
    #[serde(default)]
    pub listed_skills: Vec<String>,
    #[serde(default)]
    pub mode: SkillListMode,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum SkillListMode {
    #[default]
    Allowlist,
    Blocklist,
}

/// Runtime-only: a skill visible to AD after scanning the library + externals.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SkillEntry {
    pub name: String,
    pub path: String,
    pub source: SkillEntrySource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub scope: SkillScope,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SkillEntrySource {
    Managed,
    External,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SkillScope {
    Global,
    Project,
    None,
}

/// Result of updating a skill source.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SkillUpdateResult {
    pub source_id: String,
    pub updated: bool,
    pub before_version: String,
    pub after_version: String,
}

impl ProfileLayers {
    pub fn is_empty(&self) -> bool {
        self.shared.is_none() && self.local.is_none() && self.env.is_empty()
    }
}

impl ClaudeSettings {
    pub fn is_empty(&self) -> bool {
        self.env.is_empty()
            && self.permissions.is_none()
            && self.hooks.is_none()
            && self.model.is_none()
            && self.theme.is_none()
            && self.extra.is_empty()
    }
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
            layers: ProfileLayers::default(),
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

    #[test]
    fn v1_profile_without_layers_field_still_parses() {
        // A v0.1 profile saved before the `layers` field existed must still
        // deserialize — `layers` defaults to empty.
        let v1 = serde_json::json!({
            "id": "old",
            "displayName": "Old",
            "color": "#7C3AED",
            "createdAt": "2026-01-01T00:00:00Z",
            "updatedAt": "2026-01-01T00:00:00Z",
            "settings": { "env": { "K": "V" } }
        });
        let p: ProfileFile = serde_json::from_value(v1).unwrap();
        assert_eq!(p.id, "old");
        assert_eq!(p.settings.env.get("K").map(String::as_str), Some("V"));
        assert!(p.layers.shared.is_none());
        assert!(p.layers.local.is_none());
        assert!(p.layers.env.is_empty());
    }

    #[test]
    fn v2_profile_with_layers_round_trips() {
        let mut env = BTreeMap::new();
        env.insert("ANTHROPIC_API_KEY".into(), "sk-test".into());
        let layers = ProfileLayers {
            shared: Some(serde_json::json!({
                "permissions": { "allow": ["fs:read"] }
            })),
            local: Some(serde_json::json!({
                "model": "claude-opus-4-7",
                "statusLine": { "command": "~/.claude/statusline.sh" }
            })),
            env,
        };
        let now = chrono::Utc.with_ymd_and_hms(2026, 5, 24, 0, 0, 0).unwrap();
        let p = ProfileFile {
            id: "layered".into(),
            display_name: "Layered".into(),
            description: None,
            color: "#7C3AED".into(),
            created_at: now,
            updated_at: now,
            layers,
            settings: ClaudeSettings::default(),
        };

        let json = serde_json::to_string(&p).unwrap();
        let back: ProfileFile = serde_json::from_str(&json).unwrap();
        assert_eq!(back, p);

        // Layers serialize at the camelCase boundary.
        let v = serde_json::to_value(&p).unwrap();
        assert!(v.get("layers").is_some());
        let layers_v = v.get("layers").unwrap();
        assert!(layers_v.get("shared").is_some());
        assert!(layers_v.get("local").is_some());
        assert_eq!(
            layers_v.get("env").and_then(|e| e.get("ANTHROPIC_API_KEY")),
            Some(&Value::String("sk-test".into()))
        );
    }

    #[test]
    fn empty_layers_omitted_from_serialized_output() {
        // A profile with default-empty layers should not bloat the JSON with
        // explicit nulls / empty maps.
        let p = ProfileFile::sample();
        let v = serde_json::to_value(&p).unwrap();
        let layers = v.get("layers").expect("layers field present");
        // shared and local are None → skip_serializing_if removes them.
        assert!(layers.get("shared").is_none());
        assert!(layers.get("local").is_none());
        // env BTreeMap is empty → skip_serializing_if removes it.
        assert!(layers.get("env").is_none());
    }
}
