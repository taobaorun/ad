use std::collections::BTreeSet;

use serde_json::Value;

use crate::models::ProfileFile;

use super::{ConversionIssue, ConversionIssueKind, ConversionPreview};

pub fn convert_claude_profile_to_codex(profile: &ProfileFile) -> ConversionPreview {
    let mut target = toml::map::Map::new();
    let mut issues = Vec::new();
    let settings = serde_json::to_value(&profile.settings).unwrap_or(Value::Null);

    if let Some(model) = settings.get("model").and_then(Value::as_str) {
        target.insert("model".into(), toml::Value::String(model.into()));
    }

    for field in ["env", "permissions", "hooks", "theme"] {
        if let Some(value) = settings.get(field) {
            let non_empty = match value {
                Value::Null => false,
                Value::Object(map) => !map.is_empty(),
                Value::Array(items) => !items.is_empty(),
                _ => true,
            };
            if non_empty {
                issues.push(ConversionIssue {
                    path: field.into(),
                    kind: ConversionIssueKind::Unsupported,
                    message: format!("Claude Code field cannot be mapped to Codex config: {field}"),
                });
            }
        }
    }

    let supported = BTreeSet::from([
        "model_reasoning_effort",
        "model_reasoning_summary",
        "model_verbosity",
        "approval_policy",
        "sandbox_mode",
        "model_provider",
        "instructions",
        "developer_instructions",
        "personality",
        "features",
        "mcp_servers",
        "profiles",
        "profile",
        "notify",
        "project_root_markers",
        "project_doc_fallback_filenames",
        "skills",
        "agents",
    ]);

    if let Some(extra) = settings.as_object() {
        for (key, value) in extra {
            if ["env", "model", "permissions", "hooks", "theme"].contains(&key.as_str()) {
                continue;
            }
            if !supported.contains(key.as_str()) {
                issues.push(ConversionIssue {
                    path: key.clone(),
                    kind: ConversionIssueKind::Unsupported,
                    message: format!("Claude Code field has no confirmed Codex equivalent: {key}"),
                });
                continue;
            }
            match json_to_toml(value) {
                Ok(Some(value)) => {
                    target.insert(key.clone(), value);
                }
                Ok(None) => {}
                Err(message) => issues.push(ConversionIssue {
                    path: key.clone(),
                    kind: ConversionIssueKind::RequiresConfirmation,
                    message,
                }),
            }
        }
    }

    let target_content = toml::to_string_pretty(&toml::Value::Table(target))
        .unwrap_or_else(|err| format!("# Conversion failed: {err}\n"));

    ConversionPreview {
        source_agent_id: profile.agent_id.clone().into(),
        target_agent_id: "codex".into(),
        target_format: "toml".into(),
        target_content,
        issues,
    }
}

fn json_to_toml(value: &Value) -> Result<Option<toml::Value>, String> {
    match value {
        Value::Null => Ok(None),
        Value::Bool(value) => Ok(Some(toml::Value::Boolean(*value))),
        Value::Number(value) => {
            if let Some(integer) = value.as_i64() {
                Ok(Some(toml::Value::Integer(integer)))
            } else if let Some(float) = value.as_f64() {
                Ok(Some(toml::Value::Float(float)))
            } else {
                Err("JSON number cannot be represented in TOML".into())
            }
        }
        Value::String(value) => Ok(Some(toml::Value::String(value.clone()))),
        Value::Array(values) => {
            let mut converted = Vec::with_capacity(values.len());
            for value in values {
                let Some(value) = json_to_toml(value)? else {
                    return Err("TOML arrays cannot contain null values".into());
                };
                converted.push(value);
            }
            Ok(Some(toml::Value::Array(converted)))
        }
        Value::Object(values) => {
            let mut table = toml::map::Map::new();
            for (key, value) in values {
                if let Some(value) = json_to_toml(value)? {
                    table.insert(key.clone(), value);
                }
            }
            Ok(Some(toml::Value::Table(table)))
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::agents::ConversionIssueKind;

    use super::*;

    #[test]
    fn maps_model_and_reports_non_equivalent_claude_fields() {
        let mut profile = ProfileFile::sample();
        profile.settings.hooks = Some(serde_json::json!({"PreToolUse": []}));

        let preview = convert_claude_profile_to_codex(&profile);

        assert_eq!(preview.target_agent_id.as_str(), "codex");
        assert_eq!(preview.target_format, "toml");
        assert!(preview.target_content.contains("model = \"claude-opus-4-7\""));
        assert!(preview
            .issues
            .iter()
            .any(|issue| issue.path == "hooks" && issue.kind == ConversionIssueKind::Unsupported));
    }

    #[test]
    fn conversion_does_not_mutate_source_profile() {
        let profile = ProfileFile::sample();
        let before = profile.clone();

        let _ = convert_claude_profile_to_codex(&profile);

        assert_eq!(profile, before);
    }
}
