use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::models::ProfileFile;

use super::{
    AgentContext, ConversionIssue, ConversionIssueKind, ConversionPreview, ResourceKind,
    ResourceLocation, ResourceRef, ResourceScope, ResourceSnapshot,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactDisposition {
    Exact,
    Mapped,
    RequiresInput,
    Unsupported,
    Conflict,
    Unchanged,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConversionRiskLevel {
    Safe,
    Confirmation,
    Dangerous,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConversionResolutionKind {
    SelectTargetModel,
    SelectPermissionPreset,
    ConfirmLocalSkillSource,
    CompletePluginSetup,
    ResolveConflict,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolutionRequirement {
    pub kind: ConversionResolutionKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversionEndpoint {
    pub resource: ResourceRef,
    pub location: ResourceLocation,
}

impl From<&ResourceSnapshot> for ConversionEndpoint {
    fn from(snapshot: &ResourceSnapshot) -> Self {
        Self {
            resource: snapshot.resource.clone(),
            location: snapshot.location.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversionArtifact {
    pub id: String,
    pub kind: ResourceKind,
    pub source: ConversionEndpoint,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<ConversionEndpoint>,
    pub disposition: ArtifactDisposition,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolution: Option<ResolutionRequirement>,
    pub risk: ConversionRiskLevel,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item_count: Option<usize>,
    pub message: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversionSummary {
    pub total: usize,
    pub automatic: usize,
    pub requires_input: usize,
    pub unsupported: usize,
    pub conflicts: usize,
    pub unchanged: usize,
    pub dangerous: usize,
}

impl ConversionSummary {
    pub fn from_artifacts(artifacts: &[ConversionArtifact]) -> Self {
        let mut summary = Self {
            total: artifacts.len(),
            ..Self::default()
        };
        for artifact in artifacts {
            match artifact.disposition {
                ArtifactDisposition::Exact | ArtifactDisposition::Mapped => {
                    summary.automatic += 1;
                }
                ArtifactDisposition::RequiresInput => summary.requires_input += 1,
                ArtifactDisposition::Unsupported => summary.unsupported += 1,
                ArtifactDisposition::Conflict => summary.conflicts += 1,
                ArtifactDisposition::Unchanged => summary.unchanged += 1,
            }
            if artifact.risk == ConversionRiskLevel::Dangerous {
                summary.dangerous += 1;
            }
        }
        summary
    }
}

pub(super) struct FieldMapping {
    pub kind: ResourceKind,
    pub target_values: BTreeMap<String, toml::Value>,
    pub replace_existing: bool,
    pub disposition: ArtifactDisposition,
    pub resolution: Option<ResolutionRequirement>,
    pub risk: ConversionRiskLevel,
    pub message: String,
}

pub fn convert_claude_profile_to_codex(profile: &ProfileFile) -> ConversionPreview {
    let mut target = toml::map::Map::new();
    let mut issues = Vec::new();
    let settings = serde_json::to_value(&profile.settings).unwrap_or(Value::Null);

    if settings.get("model").and_then(Value::as_str).is_some() {
        issues.push(ConversionIssue {
            path: "model".into(),
            kind: ConversionIssueKind::RequiresConfirmation,
            message: "Claude model names have no automatic Codex equivalent; select a Codex model"
                .into(),
        });
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

    if let Some(extra) = settings.as_object() {
        for (key, value) in extra {
            if ["env", "model", "permissions", "hooks", "theme"].contains(&key.as_str()) {
                continue;
            }
            if !is_exact_codex_field(key) {
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

pub(super) fn map_claude_setting(field: &str, value: &Value) -> Option<FieldMapping> {
    if is_empty(value) {
        return None;
    }
    let kind = artifact_kind(field);
    match field {
        "env" | "hooks" | "theme" => Some(FieldMapping {
            kind,
            target_values: BTreeMap::new(),
            replace_existing: false,
            disposition: ArtifactDisposition::Unsupported,
            resolution: None,
            risk: ConversionRiskLevel::Safe,
            message: format!("Claude Code field has no confirmed Codex equivalent: {field}"),
        }),
        "permissions" => Some(FieldMapping {
            kind,
            target_values: BTreeMap::new(),
            replace_existing: false,
            disposition: ArtifactDisposition::RequiresInput,
            resolution: Some(ResolutionRequirement {
                kind: ConversionResolutionKind::SelectPermissionPreset,
            }),
            risk: ConversionRiskLevel::Confirmation,
            message: "Claude permissions require approval_policy and sandbox_mode choices".into(),
        }),
        "model" => Some(requires_input(
            kind,
            ConversionResolutionKind::SelectTargetModel,
            if value.is_string() {
                "Claude model names have no automatic Codex equivalent; select a Codex model".into()
            } else {
                "Claude model selection must be a string".into()
            },
        )),
        "maxContextTokens" => match value.as_u64().and_then(|value| i64::try_from(value).ok()) {
            Some(value) if value > 0 => Some(single_target(
                kind,
                "model_context_window",
                toml::Value::Integer(value),
                ArtifactDisposition::Mapped,
                "Claude maxContextTokens maps to the Codex model_context_window key".into(),
            )),
            _ => Some(requires_input(
                kind,
                ConversionResolutionKind::ResolveConflict,
                "Claude maxContextTokens must be a positive integer".into(),
            )),
        },
        field if is_exact_codex_field(field) => match json_to_toml(value) {
            Ok(Some(target_value)) => Some(single_target(
                kind,
                field,
                target_value,
                ArtifactDisposition::Exact,
                format!("Field {field} has a direct Codex representation"),
            )),
            Ok(None) => None,
            Err(message) => Some(requires_input(
                kind,
                ConversionResolutionKind::ResolveConflict,
                message,
            )),
        },
        _ => Some(FieldMapping {
            kind,
            target_values: BTreeMap::new(),
            replace_existing: false,
            disposition: ArtifactDisposition::Unsupported,
            resolution: None,
            risk: ConversionRiskLevel::Safe,
            message: format!("Claude Code field has no confirmed Codex equivalent: {field}"),
        }),
    }
}

pub(super) fn map_skill_artifact(
    source: &ResourceSnapshot,
    target_context: &AgentContext,
    target: Option<&ResourceSnapshot>,
    target_location: ResourceLocation,
    confirmed: bool,
) -> Option<ConversionArtifact> {
    let scope = source.content.get("scope").and_then(Value::as_str)?;
    if scope == "none" {
        return None;
    }
    let name = source.content.get("name").and_then(Value::as_str)?;
    let target_resource = target
        .map(|snapshot| snapshot.resource.clone())
        .unwrap_or_else(|| {
            collection_target(
                target_context,
                ResourceKind::Skills,
                source.resource.scope,
                name,
            )
        });
    let (disposition, resolution, message) = match target {
        Some(target) if locations_are_equivalent(source, target) => (
            ArtifactDisposition::Unchanged,
            None,
            "Target already references the same Skill source".into(),
        ),
        Some(_) => (
            ArtifactDisposition::Conflict,
            Some(ResolutionRequirement {
                kind: ConversionResolutionKind::ResolveConflict,
            }),
            "Target already has a Skill with this name from a different source".into(),
        ),
        None if confirmed => (
            ArtifactDisposition::Mapped,
            None,
            "Confirmed local Skill source will be linked into Codex".into(),
        ),
        None => (
            ArtifactDisposition::RequiresInput,
            Some(ResolutionRequirement {
                kind: ConversionResolutionKind::ConfirmLocalSkillSource,
            }),
            "Skill source must be confirmed before Codex installation".into(),
        ),
    };
    Some(ConversionArtifact {
        id: format!("skill:{name}"),
        kind: ResourceKind::Skills,
        source: ConversionEndpoint::from(source),
        target: Some(ConversionEndpoint {
            resource: target_resource,
            location: target
                .map(|snapshot| snapshot.location.clone())
                .unwrap_or(target_location),
        }),
        disposition,
        resolution,
        risk: ConversionRiskLevel::Confirmation,
        item_count: None,
        message,
    })
}

pub(super) fn map_plugin_artifact(
    source: &ResourceSnapshot,
    target_context: &AgentContext,
    target: Option<&ResourceSnapshot>,
    target_location: Option<ResourceLocation>,
) -> ConversionArtifact {
    let target_resource = target
        .map(|snapshot| snapshot.resource.clone())
        .or_else(|| {
            (source.resource.scope == ResourceScope::User).then(|| {
                collection_target(
                    target_context,
                    ResourceKind::Plugins,
                    ResourceScope::User,
                    &source.resource.logical_id,
                )
            })
        });
    let (disposition, resolution, message) = if target.is_some() {
        (
            ArtifactDisposition::Conflict,
            Some(ResolutionRequirement {
                kind: ConversionResolutionKind::CompletePluginSetup,
            }),
            "Target plugin identity exists but marketplace equivalence is not confirmed".into(),
        )
    } else {
        (
            ArtifactDisposition::Unsupported,
            None,
            "Plugin must be installed and authorized through the Codex plugin marketplace".into(),
        )
    };
    ConversionArtifact {
        id: format!("plugin:{}", source.resource.logical_id),
        kind: ResourceKind::Plugins,
        source: ConversionEndpoint::from(source),
        target: target_resource
            .zip(target_location)
            .map(|(resource, location)| ConversionEndpoint { resource, location }),
        disposition,
        resolution,
        risk: ConversionRiskLevel::Confirmation,
        item_count: None,
        message,
    }
}

fn collection_target(
    context: &AgentContext,
    kind: ResourceKind,
    scope: ResourceScope,
    logical_id: &str,
) -> ResourceRef {
    ResourceRef {
        installation_id: context.installation_id.clone(),
        project_path: (scope == ResourceScope::Project)
            .then(|| context.project_path.clone())
            .flatten(),
        kind,
        scope,
        logical_id: logical_id.into(),
    }
}

fn locations_are_equivalent(left: &ResourceSnapshot, right: &ResourceSnapshot) -> bool {
    let left = std::fs::canonicalize(&left.location.path);
    let right = std::fs::canonicalize(&right.location.path);
    matches!((left, right), (Ok(left), Ok(right)) if left == right)
}

fn is_exact_codex_field(field: &str) -> bool {
    matches!(
        field,
        "model_reasoning_effort"
            | "model_reasoning_summary"
            | "model_verbosity"
            | "approval_policy"
            | "sandbox_mode"
            | "model_provider"
            | "instructions"
            | "developer_instructions"
            | "personality"
            | "features"
            | "mcp_servers"
            | "profiles"
            | "profile"
            | "notify"
            | "project_root_markers"
            | "project_doc_fallback_filenames"
            | "skills"
            | "agents"
    )
}

fn artifact_kind(field: &str) -> ResourceKind {
    match field {
        "hooks" => ResourceKind::Hooks,
        "instructions" | "developer_instructions" => ResourceKind::Instructions,
        "mcp_servers" => ResourceKind::Mcp,
        "skills" => ResourceKind::Skills,
        "agents" => ResourceKind::Agents,
        _ => ResourceKind::Settings,
    }
}

fn requires_input(
    kind: ResourceKind,
    resolution: ConversionResolutionKind,
    message: String,
) -> FieldMapping {
    FieldMapping {
        kind,
        target_values: BTreeMap::new(),
        replace_existing: false,
        disposition: ArtifactDisposition::RequiresInput,
        resolution: Some(ResolutionRequirement { kind: resolution }),
        risk: ConversionRiskLevel::Confirmation,
        message,
    }
}

pub(super) fn explicit_targets(
    kind: ResourceKind,
    values: impl IntoIterator<Item = (String, toml::Value)>,
    message: impl Into<String>,
) -> FieldMapping {
    FieldMapping {
        kind,
        target_values: values.into_iter().collect(),
        replace_existing: true,
        disposition: ArtifactDisposition::Mapped,
        resolution: None,
        risk: ConversionRiskLevel::Confirmation,
        message: message.into(),
    }
}

fn single_target(
    kind: ResourceKind,
    key: impl Into<String>,
    value: toml::Value,
    disposition: ArtifactDisposition,
    message: String,
) -> FieldMapping {
    FieldMapping {
        kind,
        target_values: BTreeMap::from([(key.into(), value)]),
        replace_existing: false,
        disposition,
        resolution: None,
        risk: ConversionRiskLevel::Safe,
        message,
    }
}

fn is_empty(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::Array(values) => values.is_empty(),
        Value::Object(values) => values.is_empty(),
        _ => false,
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
    fn requires_a_codex_model_and_reports_non_equivalent_claude_fields() {
        let mut profile = ProfileFile::sample();
        profile.settings.hooks = Some(serde_json::json!({"PreToolUse": []}));

        let preview = convert_claude_profile_to_codex(&profile);

        assert_eq!(preview.target_agent_id.as_str(), "codex");
        assert_eq!(preview.target_format, "toml");
        assert!(!preview.target_content.contains("model ="));
        assert!(preview.issues.iter().any(|issue| {
            issue.path == "model" && issue.kind == ConversionIssueKind::RequiresConfirmation
        }));
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
