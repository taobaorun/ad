use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use chrono::{Duration, Utc};

use super::codex_ports::{
    agent_error, project_runtime_for_context, read_optional, resolve_codex_home,
};
use super::{
    load_project_codex_runtime_manifest, render_project_codex_runtime_manifest, AgentContext,
    AgentError, AgentErrorCode, AgentId, ContentDigest, MutationKind, MutationPlan, PlanId,
    PlannedMutation, ReadPrecondition, ResourceKind, ResourceRef, ResourceScope, WritePolicy,
};

pub(super) fn disabled_skill_paths(context: &AgentContext) -> Result<BTreeSet<String>, AgentError> {
    let config = resolve_codex_home(context)?.join("config.toml");
    let Some(bytes) = read_optional(&config, context, None)? else {
        return Ok(BTreeSet::new());
    };
    let value = std::str::from_utf8(&bytes)
        .map_err(|error| {
            agent_error(
                AgentErrorCode::InvalidPlan,
                context,
                None,
                error.to_string(),
            )
        })?
        .parse::<toml::Value>()
        .map_err(|error| {
            agent_error(
                AgentErrorCode::InvalidPlan,
                context,
                None,
                error.to_string(),
            )
        })?;
    Ok(value
        .get("skills")
        .and_then(|skills| skills.get("config"))
        .and_then(toml::Value::as_array)
        .into_iter()
        .flatten()
        .filter(|entry| entry.get("enabled").and_then(toml::Value::as_bool) == Some(false))
        .filter_map(|entry| entry.get("path").and_then(toml::Value::as_str))
        .map(|path| normalize_skill_config_path(&config, path))
        .collect())
}

fn normalize_skill_config_path(config: &Path, path: &str) -> String {
    let path = PathBuf::from(path);
    let resolved = if path.is_absolute() {
        path
    } else {
        config.parent().unwrap_or_else(|| Path::new(".")).join(path)
    };
    std::fs::canonicalize(&resolved)
        .unwrap_or(resolved)
        .to_string_lossy()
        .into_owned()
}

pub(super) fn plan_skill_config(
    context: &AgentContext,
    skill_md: &Path,
    enabled: bool,
) -> Result<MutationPlan, AgentError> {
    let config = resolve_codex_home(context)?.join("config.toml");
    let existing = read_optional(&config, context, None)?;
    let mut value = match existing.as_deref() {
        Some(bytes) => std::str::from_utf8(bytes)
            .map_err(|error| {
                agent_error(
                    AgentErrorCode::InvalidPlan,
                    context,
                    None,
                    error.to_string(),
                )
            })?
            .parse::<toml::Value>()
            .map_err(|error| {
                agent_error(
                    AgentErrorCode::InvalidPlan,
                    context,
                    None,
                    error.to_string(),
                )
            })?,
        None => toml::Value::Table(toml::map::Map::new()),
    };
    let root = value.as_table_mut().ok_or_else(|| {
        agent_error(
            AgentErrorCode::InvalidPlan,
            context,
            None,
            "Codex config must be a TOML table",
        )
    })?;
    let skills = root
        .entry("skills")
        .or_insert_with(|| toml::Value::Table(toml::map::Map::new()))
        .as_table_mut()
        .ok_or_else(|| {
            agent_error(
                AgentErrorCode::InvalidPlan,
                context,
                None,
                "skills must be a TOML table",
            )
        })?;
    let configs = skills
        .entry("config")
        .or_insert_with(|| toml::Value::Array(Vec::new()))
        .as_array_mut()
        .ok_or_else(|| {
            agent_error(
                AgentErrorCode::InvalidPlan,
                context,
                None,
                "skills.config must be an array",
            )
        })?;
    let path = skill_md.to_string_lossy().into_owned();
    configs.retain(
        |entry| match entry.get("path").and_then(toml::Value::as_str) {
            Some(entry_path) => normalize_skill_config_path(&config, entry_path) != path,
            None => true,
        },
    );
    if !enabled {
        configs.push(toml::Value::Table(toml::map::Map::from_iter([
            ("path".into(), toml::Value::String(path)),
            ("enabled".into(), toml::Value::Boolean(false)),
        ])));
    }
    let rendered = toml::to_string_pretty(&value).map_err(|error| {
        agent_error(
            AgentErrorCode::InvalidPlan,
            context,
            None,
            error.to_string(),
        )
    })?;
    let expected_digest = existing.as_deref().map(ContentDigest::sha256);
    let runtime = project_runtime_for_context(context)?;
    let resource = if runtime.is_some() {
        ResourceRef {
            installation_id: context.installation_id.clone(),
            project_path: context.project_path.clone(),
            kind: ResourceKind::Settings,
            scope: ResourceScope::Project,
            logical_id: "runtime-config".into(),
        }
    } else {
        ResourceRef {
            installation_id: context.installation_id.clone(),
            project_path: None,
            kind: ResourceKind::Settings,
            scope: ResourceScope::User,
            logical_id: "user-config".into(),
        }
    };
    let mut plan = config_plan(context, resource, rendered, expected_digest);
    if let Some(runtime) = runtime {
        let snapshot = load_project_codex_runtime_manifest(&runtime)
            .map_err(|error| {
                agent_error(
                    AgentErrorCode::InvalidPlan,
                    context,
                    None,
                    error.to_string(),
                )
            })?
            .ok_or_else(|| {
                agent_error(
                    AgentErrorCode::ResourceChanged,
                    context,
                    None,
                    "Project Skill toggles require an applied runtime manifest",
                )
            })?;
        let mut manifest = snapshot.manifest;
        if manifest.project_settings_keys.insert("skills".into()) {
            let rendered = render_project_codex_runtime_manifest(&manifest).map_err(|error| {
                agent_error(
                    AgentErrorCode::InvalidPlan,
                    context,
                    None,
                    error.to_string(),
                )
            })?;
            let content = serde_json::from_slice(&rendered).map_err(|error| {
                agent_error(
                    AgentErrorCode::InvalidPlan,
                    context,
                    None,
                    error.to_string(),
                )
            })?;
            plan.mutations.insert(
                0,
                PlannedMutation {
                    resource: ResourceRef {
                        installation_id: context.installation_id.clone(),
                        project_path: context.project_path.clone(),
                        kind: ResourceKind::Plugins,
                        scope: ResourceScope::Project,
                        logical_id: "runtime-manifest".into(),
                    },
                    kind: MutationKind::Replace,
                    expected_digest: Some(snapshot.digest),
                    media_type: "application/json".into(),
                    content: Some(content),
                },
            );
        }
    }
    Ok(plan)
}

fn config_plan(
    context: &AgentContext,
    resource: ResourceRef,
    content: String,
    expected_digest: Option<ContentDigest>,
) -> MutationPlan {
    MutationPlan {
        id: PlanId::from(uuid::Uuid::new_v4().to_string()),
        agent_id: AgentId::from("codex"),
        context: context.clone(),
        read_set: expected_digest
            .clone()
            .map(|digest| {
                vec![ReadPrecondition {
                    resource: resource.clone(),
                    expected_digest: digest,
                    write_policy: WritePolicy::Mutable,
                }]
            })
            .unwrap_or_default(),
        mutations: vec![PlannedMutation {
            resource,
            kind: if expected_digest.is_some() {
                MutationKind::Replace
            } else {
                MutationKind::Create
            },
            expected_digest,
            media_type: "application/toml".into(),
            content: Some(serde_json::Value::String(content)),
        }],
        expires_at: Utc::now() + Duration::minutes(5),
    }
}
