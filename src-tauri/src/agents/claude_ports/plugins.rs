use std::collections::{BTreeMap, BTreeSet};

use chrono::{Duration, Utc};

use crate::fs::paths::managed_collection_runtime_dir;

use super::super::{
    AgentContext, AgentError, AgentErrorCode, AgentId, CapabilityAvailability,
    CapabilityLimitation, CapabilityOperation, CollectionInstallRequest, ContentDigest,
    ManagedResourceTarget, MutationKind, MutationPlan, PlanId, PlannedMutation, PluginsPort,
    ReadPrecondition, ResourceKind, ResourceLocation, ResourceOrigin, ResourcePort, ResourceRef,
    ResourceScope, ResourceSnapshot, WritePolicy,
};
use super::common::{agent_error, read_optional, resolve_claude_home, validate_project_path};

#[derive(Debug, Default)]
pub(crate) struct ClaudePluginsPort;

impl ResourcePort for ClaudePluginsPort {
    fn resolve(
        &self,
        context: &AgentContext,
        resource: &ResourceRef,
    ) -> Result<ManagedResourceTarget, AgentError> {
        if resource.installation_id != context.installation_id
            || resource.kind != ResourceKind::Plugins
        {
            return Err(agent_error(
                AgentErrorCode::InvalidPlan,
                context,
                Some(resource.clone()),
                "Plugin resource does not belong to the active Agent context",
            ));
        }
        let managed_project = resource.scope == ResourceScope::Project
            && resource.logical_id.starts_with("skill-source:");
        let path = match resource.scope {
            ResourceScope::User if resource.project_path.is_none() => {
                resolve_claude_home(context)?.join("settings.json")
            }
            ResourceScope::Project
                if context.project_path.is_some()
                    && resource.project_path == context.project_path =>
            {
                if managed_project {
                    managed_plugin_link(context, &resource.logical_id)?
                } else if let Some(id) = resource.logical_id.strip_prefix("plugin-control:") {
                    super::super::installation_control_path(
                        &super::super::ResourceInstallationId::from(id.to_owned()),
                    )
                    .map_err(|error| {
                        agent_error(
                            AgentErrorCode::Io,
                            context,
                            Some(resource.clone()),
                            error.to_string(),
                        )
                    })?
                } else {
                    validate_project_path(
                        context,
                        context.project_path.as_deref().unwrap_or_default(),
                    )?
                    .join(".claude/settings.local.json")
                }
            }
            _ => {
                return Err(agent_error(
                    AgentErrorCode::InvalidPlan,
                    context,
                    Some(resource.clone()),
                    "Plugin scope does not belong to the active Agent context",
                ))
            }
        };
        Ok(if managed_project {
            ManagedResourceTarget::symlink(path)
        } else {
            ManagedResourceTarget::file(path)
        })
    }
}

impl PluginsPort for ClaudePluginsPort {
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

    fn limitations(&self) -> Vec<CapabilityLimitation> {
        Vec::new()
    }

    fn list(&self, context: &AgentContext) -> Result<Vec<ResourceSnapshot>, AgentError> {
        let claude_home = resolve_claude_home(context)?;
        let (scope, project_path, plugins) = if let Some(project_path) = &context.project_path {
            let project = validate_project_path(context, project_path)?;
            let user = claude_home.join("settings.json");
            let shared = project.join(".claude/settings.json");
            let local = project.join(".claude/settings.local.json");
            let mut plugins = read_declared_plugins(context, &user)?
                .into_iter()
                .map(|(id, enabled)| (id, (enabled, user.clone(), ResourceOrigin::User)))
                .collect::<BTreeMap<_, _>>();
            plugins.extend(
                read_declared_plugins(context, &shared)?
                    .into_iter()
                    .map(|(id, enabled)| (id, (enabled, shared.clone(), ResourceOrigin::Project))),
            );
            plugins.extend(
                read_declared_plugins(context, &local)?
                    .into_iter()
                    .map(|(id, enabled)| (id, (enabled, local.clone(), ResourceOrigin::Project))),
            );
            (ResourceScope::Project, Some(project_path.clone()), plugins)
        } else {
            let location = claude_home.join("settings.json");
            let plugins = read_declared_plugins(context, &location)?
                .into_iter()
                .map(|(id, enabled)| (id, (enabled, location.clone(), ResourceOrigin::User)))
                .collect();
            (ResourceScope::User, None, plugins)
        };
        plugins
            .into_iter()
            .map(|(plugin_id, (enabled, location, origin))| {
                let content = serde_json::json!({
                    "id": plugin_id.clone(),
                    "enabled": enabled,
                    "source": "external",
                });
                let bytes = serde_json::to_vec(&content).map_err(|error| {
                    agent_error(
                        AgentErrorCode::Io,
                        context,
                        None,
                        format!("Failed to digest Claude plugin {plugin_id}: {error}"),
                    )
                })?;
                Ok(ResourceSnapshot {
                    resource: ResourceRef {
                        installation_id: context.installation_id.clone(),
                        project_path: project_path.clone(),
                        kind: ResourceKind::Plugins,
                        scope,
                        logical_id: plugin_id,
                    },
                    location: ResourceLocation {
                        path: location.to_string_lossy().into_owned(),
                        origin,
                    },
                    media_type: "application/vnd.ad.plugin+json".into(),
                    content,
                    digest: ContentDigest::sha256(&bytes),
                    observed_at: Utc::now(),
                })
            })
            .collect()
    }

    fn plan_install(
        &self,
        context: &AgentContext,
        request: CollectionInstallRequest,
    ) -> Result<MutationPlan, AgentError> {
        let project_path = context.project_path.clone().ok_or_else(|| {
            agent_error(
                AgentErrorCode::InvalidPlan,
                context,
                None,
                "Claude Plugin installation requires a project",
            )
        })?;
        validate_project_path(context, &project_path)?;
        let resource_id = request
            .source
            .get("catalogResourceId")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| {
                agent_error(
                    AgentErrorCode::InvalidPlan,
                    context,
                    None,
                    "Project Plugin installation requires a catalog resource",
                )
            })?;
        let resolved = super::super::resolve_catalog_resource(resource_id).map_err(|error| {
            agent_error(
                AgentErrorCode::InvalidPlan,
                context,
                None,
                error.to_string(),
            )
        })?;
        if resolved.kind != ResourceKind::Plugins {
            return Err(agent_error(
                AgentErrorCode::InvalidPlan,
                context,
                None,
                "Catalog resource is not a Plugin",
            ));
        }
        if !resolved
            .physical_path
            .join(".claude-plugin/plugin.json")
            .is_file()
        {
            return Err(agent_error(
                AgentErrorCode::Unsupported,
                context,
                None,
                "This Plugin does not declare Claude Code support",
            ));
        }
        let resource = ResourceRef {
            installation_id: context.installation_id.clone(),
            project_path: Some(project_path),
            kind: ResourceKind::Plugins,
            scope: ResourceScope::Project,
            logical_id: format!("{}/{}", resolved.source_id, resolved.install_id),
        };
        let target = self.resolve(context, &resource)?.path().to_path_buf();
        match std::fs::symlink_metadata(&target) {
            Ok(_) => {
                return Err(agent_error(
                    AgentErrorCode::PermissionDenied,
                    context,
                    Some(resource),
                    "Claude Plugin target already exists; uninstall it before installing another source",
                ))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(agent_error(
                    AgentErrorCode::Io,
                    context,
                    Some(resource),
                    error.to_string(),
                ))
            }
        }
        let digest =
            super::super::directory_tree_digest(&resolved.stable_path).map_err(|error| {
                agent_error(
                    AgentErrorCode::InvalidPlan,
                    context,
                    None,
                    error.to_string(),
                )
            })?;
        Ok(MutationPlan {
            id: PlanId::from(uuid::Uuid::new_v4().to_string()),
            agent_id: AgentId::from("claude-code"),
            context: context.clone(),
            read_set: Vec::new(),
            mutations: vec![PlannedMutation {
                resource,
                kind: MutationKind::Create,
                expected_digest: None,
                media_type: "application/vnd.ad.symlink".into(),
                content: Some(serde_json::json!({
                    "path": resolved.stable_path,
                    "digest": digest,
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
        if resource.logical_id.starts_with("skill-source:") {
            let record = super::super::list_resource_installations()
                .map_err(|error| agent_error(AgentErrorCode::Io, context, None, error.to_string()))?
                .into_iter()
                .find(|record| {
                    record.effective_installation_id == context.installation_id
                        && record.canonical_project_path
                            == context.project_path.as_deref().unwrap_or_default()
                        && format!("{}/{}", record.source_id, record.install_id)
                            == resource.logical_id
                })
                .ok_or_else(|| {
                    agent_error(
                        AgentErrorCode::ResourceChanged,
                        context,
                        Some(resource.clone()),
                        "Managed Plugin installation evidence is unavailable",
                    )
                })?;
            let path = super::super::installation_control_path(&record.id).map_err(|error| {
                agent_error(
                    AgentErrorCode::Io,
                    context,
                    Some(resource.clone()),
                    error.to_string(),
                )
            })?;
            let current = std::fs::read(&path).ok();
            let expected_digest = current.as_deref().map(ContentDigest::sha256);
            let control_resource = ResourceRef {
                logical_id: format!("plugin-control:{}", record.id),
                ..resource.clone()
            };
            return Ok(MutationPlan {
                id: PlanId::from(uuid::Uuid::new_v4().to_string()),
                agent_id: AgentId::from("claude-code"),
                context: context.clone(),
                read_set: Vec::new(),
                mutations: vec![PlannedMutation {
                    resource: control_resource,
                    kind: if current.is_some() {
                        MutationKind::Replace
                    } else {
                        MutationKind::Create
                    },
                    expected_digest,
                    media_type: "application/json".into(),
                    content: Some(super::super::installation_control_content(enabled)),
                }],
                expires_at: Utc::now() + Duration::minutes(5),
            });
        }
        plan_project_override(context, resource, Some(enabled))
    }

    fn plan_remove(
        &self,
        context: &AgentContext,
        resource: &ResourceRef,
    ) -> Result<MutationPlan, AgentError> {
        if resource.logical_id.starts_with("skill-source:") {
            let target = self.resolve(context, resource)?.path().to_path_buf();
            let metadata = std::fs::symlink_metadata(&target).map_err(|error| {
                agent_error(
                    AgentErrorCode::ResourceChanged,
                    context,
                    Some(resource.clone()),
                    error.to_string(),
                )
            })?;
            if !metadata.file_type().is_symlink() {
                return Err(agent_error(
                    AgentErrorCode::PermissionDenied,
                    context,
                    Some(resource.clone()),
                    "Managed Claude Plugin target is not a symlink",
                ));
            }
            let digest = ContentDigest::sha256(
                std::fs::read_link(&target)
                    .map_err(|error| {
                        agent_error(
                            AgentErrorCode::Io,
                            context,
                            Some(resource.clone()),
                            error.to_string(),
                        )
                    })?
                    .to_string_lossy()
                    .as_bytes(),
            );
            let mut mutations = Vec::new();
            let record = super::super::list_resource_installations()
                .map_err(|error| agent_error(AgentErrorCode::Io, context, None, error.to_string()))?
                .into_iter()
                .find(|record| {
                    record.effective_installation_id == context.installation_id
                        && record.canonical_project_path
                            == context.project_path.as_deref().unwrap_or_default()
                        && format!("{}/{}", record.source_id, record.install_id)
                            == resource.logical_id
                });
            if let Some(record) = record {
                let control_resource = ResourceRef {
                    logical_id: format!("plugin-control:{}", record.id),
                    ..resource.clone()
                };
                let control_path = self
                    .resolve(context, &control_resource)?
                    .path()
                    .to_path_buf();
                match std::fs::read(&control_path) {
                    Ok(bytes) => mutations.push(PlannedMutation {
                        resource: control_resource,
                        kind: MutationKind::Delete,
                        expected_digest: Some(ContentDigest::sha256(&bytes)),
                        media_type: "application/json".into(),
                        content: None,
                    }),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => {
                        return Err(agent_error(
                            AgentErrorCode::Io,
                            context,
                            Some(resource.clone()),
                            error.to_string(),
                        ))
                    }
                }
            }
            mutations.push(PlannedMutation {
                resource: resource.clone(),
                kind: MutationKind::Delete,
                expected_digest: Some(digest.clone()),
                media_type: "application/vnd.ad.symlink".into(),
                content: None,
            });
            return Ok(MutationPlan {
                id: PlanId::from(uuid::Uuid::new_v4().to_string()),
                agent_id: AgentId::from("claude-code"),
                context: context.clone(),
                read_set: vec![ReadPrecondition {
                    resource: resource.clone(),
                    expected_digest: digest.clone(),
                    write_policy: WritePolicy::Mutable,
                }],
                mutations,
                expires_at: Utc::now() + Duration::minutes(5),
            });
        }
        plan_project_override(context, resource, None)
    }
}

pub(crate) fn managed_claude_plugin_links(
    context: &AgentContext,
) -> Result<Vec<std::path::PathBuf>, AgentError> {
    let project_path = context.project_path.as_deref().ok_or_else(|| {
        agent_error(
            AgentErrorCode::InvalidPlan,
            context,
            None,
            "Project is required",
        )
    })?;
    validate_project_path(context, project_path)?;
    let records = super::super::list_resource_installations()
        .map_err(|error| agent_error(AgentErrorCode::Io, context, None, error.to_string()))?;
    let mut links = records
        .into_iter()
        .filter(|record| {
            record.effective_installation_id == context.installation_id
                && record.canonical_project_path == project_path
                && record.resource_kind == ResourceKind::Plugins
                && record.adapter_contract == "claude-plugin-dir-v1"
        })
        .filter_map(|record| match super::super::installation_enabled(&record) {
            Ok(true) => Some(Ok(record)),
            Ok(false) => None,
            Err(error) => Some(Err(agent_error(
                AgentErrorCode::Io,
                context,
                None,
                error.to_string(),
            ))),
        })
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .map(|record| {
            managed_plugin_link(
                context,
                &format!("{}/{}", record.source_id, record.install_id),
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    links.sort();
    Ok(links)
}

fn managed_plugin_link(
    context: &AgentContext,
    logical_id: &str,
) -> Result<std::path::PathBuf, AgentError> {
    let project = context.project_path.as_deref().ok_or_else(|| {
        agent_error(
            AgentErrorCode::InvalidPlan,
            context,
            None,
            "Project is required",
        )
    })?;
    let workspace = super::super::resolve_project_agent_workspace(
        &context.installation_id,
        std::path::Path::new(project),
    )?;
    let install_id = logical_id.rsplit('/').next().unwrap_or(logical_id);
    let key =
        super::super::opaque_contract_id("plugin-link", &[workspace.key.as_str(), install_id])
            .replace(':', "_");
    managed_collection_runtime_dir()
        .map(|runtime| runtime.join(key))
        .map_err(|error| agent_error(AgentErrorCode::Io, context, None, error.to_string()))
}

fn plan_project_override(
    context: &AgentContext,
    resource: &ResourceRef,
    enabled: Option<bool>,
) -> Result<MutationPlan, AgentError> {
    if resource.installation_id != context.installation_id || resource.kind != ResourceKind::Plugins
    {
        return Err(agent_error(
            AgentErrorCode::InvalidPlan,
            context,
            Some(resource.clone()),
            "Plugin resource does not belong to the active Agent context",
        ));
    }
    let project_path = context.project_path.as_deref().ok_or_else(|| {
        agent_error(
            AgentErrorCode::InvalidPlan,
            context,
            Some(resource.clone()),
            "Claude plugin override requires a project context",
        )
    })?;
    let project = validate_project_path(context, project_path)?;
    let target = project.join(".claude/settings.local.json");
    let existing = read_optional(&target, context, Some(resource.clone()))?;
    let expected_digest = existing.as_deref().map(ContentDigest::sha256);
    let mut content = match existing.as_deref() {
        Some(bytes) => serde_json::from_slice(bytes).map_err(|error| {
            agent_error(
                AgentErrorCode::InvalidPlan,
                context,
                Some(resource.clone()),
                format!(
                    "Invalid project settings JSON at {}: {error}",
                    target.display()
                ),
            )
        })?,
        None => serde_json::json!({}),
    };
    let object = content.as_object_mut().ok_or_else(|| {
        agent_error(
            AgentErrorCode::InvalidPlan,
            context,
            Some(resource.clone()),
            "Project settings JSON must be an object",
        )
    })?;
    let plugins = object
        .entry("enabledPlugins")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .ok_or_else(|| {
            agent_error(
                AgentErrorCode::InvalidPlan,
                context,
                Some(resource.clone()),
                "enabledPlugins must be a JSON object",
            )
        })?;
    match enabled {
        Some(enabled) => {
            plugins.insert(
                resource.logical_id.clone(),
                serde_json::Value::Bool(enabled),
            );
        }
        None if plugins.remove(&resource.logical_id).is_none() => {
            return Err(agent_error(
                AgentErrorCode::ResourceChanged,
                context,
                Some(resource.clone()),
                "Project Plugin override is no longer present",
            ));
        }
        None => {}
    }
    let read_set = expected_digest
        .clone()
        .map(|digest| {
            vec![ReadPrecondition {
                resource: resource.clone(),
                expected_digest: digest,
                write_policy: WritePolicy::Mutable,
            }]
        })
        .unwrap_or_default();

    Ok(MutationPlan {
        id: PlanId::from(uuid::Uuid::new_v4().to_string()),
        agent_id: AgentId::from("claude-code"),
        context: context.clone(),
        read_set,
        mutations: vec![PlannedMutation {
            resource: resource.clone(),
            kind: if expected_digest.is_some() {
                MutationKind::Replace
            } else {
                MutationKind::Create
            },
            expected_digest,
            media_type: "application/json".into(),
            content: Some(content),
        }],
        expires_at: Utc::now() + Duration::minutes(5),
    })
}

fn read_declared_plugins(
    context: &AgentContext,
    location: &std::path::PathBuf,
) -> Result<BTreeMap<String, bool>, AgentError> {
    let Some(bytes) = read_optional(location, context, None)? else {
        return Ok(BTreeMap::new());
    };
    let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(|error| {
        agent_error(
            AgentErrorCode::InvalidPlan,
            context,
            None,
            format!(
                "Invalid Claude settings JSON at {}: {error}",
                location.display()
            ),
        )
    })?;
    let Some(plugins) = value.get("enabledPlugins") else {
        return Ok(BTreeMap::new());
    };
    let plugins = plugins.as_object().ok_or_else(|| {
        agent_error(
            AgentErrorCode::InvalidPlan,
            context,
            None,
            format!("enabledPlugins must be an object in {}", location.display()),
        )
    })?;
    Ok(plugins
        .iter()
        .map(|(id, enabled)| (id.clone(), enabled.as_bool().unwrap_or(false)))
        .collect())
}
