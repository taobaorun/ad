use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};

use chrono::{Duration, Utc};

use crate::fs::paths::home;

use super::codex_ports::{
    agent_error, project_runtime_for_context, read_optional, resolve_codex_home,
    validate_project_path,
};
use super::execution_fs::directory_tree_digest;
use super::{
    load_project_codex_runtime_manifest, render_project_codex_runtime_manifest, AgentContext,
    AgentError, AgentErrorCode, AgentId, CapabilityAvailability, CapabilityOperation,
    CollectionInstallRequest, ContentDigest, ManagedResourceTarget, MutationKind, MutationPlan,
    PlanId, PlannedMutation, ReadPrecondition, ResourceKind, ResourceLocation, ResourceOrigin,
    ResourcePort, ResourceRef, ResourceScope, ResourceSnapshot, SkillsPort, WritePolicy,
};

#[derive(Debug, Default)]
pub(crate) struct CodexSkillsPort;

impl ResourcePort for CodexSkillsPort {
    fn resolve(
        &self,
        context: &AgentContext,
        resource: &ResourceRef,
    ) -> Result<ManagedResourceTarget, AgentError> {
        validate_resource(context, resource)?;
        Ok(ManagedResourceTarget::symlink(
            skill_root(context, resource.scope)?.join(&resource.logical_id),
        ))
    }
}

impl SkillsPort for CodexSkillsPort {
    fn scopes(&self) -> BTreeSet<ResourceScope> {
        BTreeSet::from([ResourceScope::User, ResourceScope::Project])
    }

    fn operations(&self) -> BTreeSet<CapabilityOperation> {
        BTreeSet::from([
            CapabilityOperation::List,
            CapabilityOperation::Install,
            CapabilityOperation::Enable,
            CapabilityOperation::Disable,
            CapabilityOperation::Preview,
            CapabilityOperation::Apply,
            CapabilityOperation::Rollback,
        ])
    }

    fn availability(&self) -> CapabilityAvailability {
        CapabilityAvailability::Available
    }

    fn list(&self, context: &AgentContext) -> Result<Vec<ResourceSnapshot>, AgentError> {
        resolve_codex_home(context)?;
        let disabled = disabled_skill_paths(context)?;
        let mut snapshots = scan_scope(context, ResourceScope::User, &disabled)?;
        if context.project_path.is_some() {
            snapshots.extend(scan_scope(context, ResourceScope::Project, &disabled)?);
        }
        Ok(snapshots)
    }

    fn plan_install(
        &self,
        context: &AgentContext,
        request: CollectionInstallRequest,
    ) -> Result<MutationPlan, AgentError> {
        validate_name(context, &request.logical_id)?;
        let source_path = request
            .source
            .get("path")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| {
                agent_error(
                    AgentErrorCode::InvalidPlan,
                    context,
                    None,
                    "Codex local skill install requires source.path",
                )
            })?;
        let source = std::fs::canonicalize(source_path).map_err(|error| {
            agent_error(
                AgentErrorCode::InvalidPlan,
                context,
                None,
                format!("Invalid skill source path {source_path}: {error}"),
            )
        })?;
        if !source.is_dir() || !source.join("SKILL.md").is_file() {
            return Err(agent_error(
                AgentErrorCode::InvalidPlan,
                context,
                None,
                "Codex skill source must be a directory containing SKILL.md",
            ));
        }
        let scope = if context.project_path.is_some() {
            ResourceScope::Project
        } else {
            ResourceScope::User
        };
        let resource = ResourceRef {
            installation_id: context.installation_id.clone(),
            project_path: (scope == ResourceScope::Project)
                .then(|| context.project_path.clone())
                .flatten(),
            kind: ResourceKind::Skills,
            scope,
            logical_id: request.logical_id,
        };
        let target = skill_root(context, scope)?.join(&resource.logical_id);
        if std::fs::symlink_metadata(&target).is_ok() {
            return Err(agent_error(
                AgentErrorCode::PermissionDenied,
                context,
                Some(resource),
                "Codex skill target already exists",
            ));
        }
        let source_digest = directory_tree_digest(&source).map_err(|error| {
            agent_error(
                AgentErrorCode::InvalidPlan,
                context,
                None,
                format!("Failed to digest Codex skill source: {error}"),
            )
        })?;
        Ok(MutationPlan {
            id: PlanId::from(uuid::Uuid::new_v4().to_string()),
            agent_id: AgentId::from("codex"),
            context: context.clone(),
            read_set: Vec::new(),
            mutations: vec![PlannedMutation {
                resource,
                kind: MutationKind::Create,
                expected_digest: None,
                media_type: "application/vnd.ad.symlink".into(),
                content: Some(serde_json::json!({
                    "path": source,
                    "digest": source_digest,
                })),
            }],
            expires_at: Utc::now() + Duration::minutes(5),
        })
    }

    fn plan_set_enabled(
        &self,
        context: &AgentContext,
        resource: &ResourceRef,
        enabled: bool,
    ) -> Result<MutationPlan, AgentError> {
        validate_resource(context, resource)?;
        let skill_md = skill_root(context, resource.scope)?
            .join(&resource.logical_id)
            .join("SKILL.md");
        let skill_md = std::fs::canonicalize(&skill_md).map_err(|error| {
            agent_error(
                AgentErrorCode::InvalidPlan,
                context,
                Some(resource.clone()),
                format!("Invalid Codex skill path {}: {error}", skill_md.display()),
            )
        })?;
        plan_skill_config(context, &skill_md, enabled)
    }
}

fn scan_scope(
    context: &AgentContext,
    scope: ResourceScope,
    disabled: &BTreeSet<String>,
) -> Result<Vec<ResourceSnapshot>, AgentError> {
    let root = skill_root(context, scope)?;
    let entries = match std::fs::read_dir(&root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(agent_error(
                AgentErrorCode::Io,
                context,
                None,
                format!("Failed to scan Codex skills at {}: {error}", root.display()),
            ))
        }
    };
    let mut snapshots = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let skill_md = path.join("SKILL.md");
        if !skill_md.is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        let canonical_md = std::fs::canonicalize(&skill_md).map_err(|error| {
            agent_error(
                AgentErrorCode::Io,
                context,
                None,
                format!("Failed to resolve {}: {error}", skill_md.display()),
            )
        })?;
        let enabled = !disabled.contains(&canonical_md.to_string_lossy().into_owned());
        let content = serde_json::json!({"name": name, "enabled": enabled});
        let bytes = serde_json::to_vec(&content)
            .map_err(|error| agent_error(AgentErrorCode::Io, context, None, error.to_string()))?;
        snapshots.push(ResourceSnapshot {
            resource: ResourceRef {
                installation_id: context.installation_id.clone(),
                project_path: (scope == ResourceScope::Project)
                    .then(|| context.project_path.clone())
                    .flatten(),
                kind: ResourceKind::Skills,
                scope,
                logical_id: name,
            },
            location: ResourceLocation {
                path: path.to_string_lossy().into_owned(),
                origin: if scope == ResourceScope::Project {
                    ResourceOrigin::Project
                } else {
                    ResourceOrigin::User
                },
            },
            media_type: "application/vnd.ad.skill+json".into(),
            content,
            digest: ContentDigest::sha256(&bytes),
            observed_at: Utc::now(),
        });
    }
    snapshots.sort_by(|left, right| left.resource.logical_id.cmp(&right.resource.logical_id));
    Ok(snapshots)
}

fn disabled_skill_paths(context: &AgentContext) -> Result<BTreeSet<String>, AgentError> {
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

fn plan_skill_config(
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

fn skill_root(context: &AgentContext, scope: ResourceScope) -> Result<PathBuf, AgentError> {
    match scope {
        ResourceScope::User => home()
            .map(|home| home.join(".agents/skills"))
            .map_err(|error| agent_error(AgentErrorCode::Io, context, None, error.to_string())),
        ResourceScope::Project => {
            validate_project_path(context, context.project_path.as_deref().unwrap_or_default())
                .map(|project| project.join(".agents/skills"))
        }
    }
}

fn validate_resource(context: &AgentContext, resource: &ResourceRef) -> Result<(), AgentError> {
    validate_name(context, &resource.logical_id)?;
    let scope_matches = match resource.scope {
        ResourceScope::User => resource.project_path.is_none(),
        ResourceScope::Project => {
            context.project_path.is_some() && resource.project_path == context.project_path
        }
    };
    if resource.installation_id != context.installation_id
        || resource.kind != ResourceKind::Skills
        || !scope_matches
    {
        return Err(agent_error(
            AgentErrorCode::InvalidPlan,
            context,
            Some(resource.clone()),
            "Skill resource does not belong to the active Agent context",
        ));
    }
    Ok(())
}

fn validate_name(context: &AgentContext, name: &str) -> Result<(), AgentError> {
    let mut components = Path::new(name).components();
    if name.is_empty()
        || !matches!(components.next(), Some(Component::Normal(_)))
        || components.next().is_some()
    {
        return Err(agent_error(
            AgentErrorCode::InvalidPlan,
            context,
            None,
            "Invalid Codex skill logical id",
        ));
    }
    Ok(())
}
