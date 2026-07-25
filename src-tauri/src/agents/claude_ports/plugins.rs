use std::collections::{BTreeMap, BTreeSet};

use chrono::{Duration, Utc};

use super::super::{
    classify_claude_plugin, inspect_claude_plugin, AgentContext, AgentError, AgentErrorCode,
    AgentId, CapabilityAvailability, CapabilityLimitation, CapabilityOperation,
    CollectionInstallRequest, ContentDigest, ManagedResourceTarget, MutationKind, MutationPlan,
    PlanId, PlannedMutation, PluginsPort, ReadPrecondition, ResourceKind, ResourceLocation,
    ResourceOrigin, ResourcePort, ResourceRef, ResourceScope, ResourceSnapshot, WritePolicy,
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
        let path = match resource.scope {
            ResourceScope::User if resource.project_path.is_none() => {
                resolve_claude_home(context)?.join("settings.json")
            }
            ResourceScope::Project
                if context.project_path.is_some()
                    && resource.project_path == context.project_path =>
            {
                validate_project_path(context, context.project_path.as_deref().unwrap_or_default())?
                    .join(".claude/settings.local.json")
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
        Ok(ManagedResourceTarget::file(path))
    }
}

impl PluginsPort for ClaudePluginsPort {
    fn scopes(&self) -> BTreeSet<ResourceScope> {
        BTreeSet::from([ResourceScope::User, ResourceScope::Project])
    }

    fn operations(&self) -> BTreeSet<CapabilityOperation> {
        BTreeSet::from([
            CapabilityOperation::List,
            CapabilityOperation::Enable,
            CapabilityOperation::Disable,
            CapabilityOperation::Preview,
            CapabilityOperation::Apply,
            CapabilityOperation::Rollback,
        ])
    }

    fn availability(&self) -> CapabilityAvailability {
        CapabilityAvailability::Degraded
    }

    fn limitations(&self) -> Vec<CapabilityLimitation> {
        vec![CapabilityLimitation {
            code: "plugin_install_not_managed".into(),
            message_key: "agents.capabilities.pluginInstallNotManaged".into(),
        }]
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
                let mut content = serde_json::json!({
                    "id": plugin_id.clone(),
                    "enabled": enabled,
                    "classification": classify_claude_plugin(None, enabled),
                });
                if enabled {
                    if let Some(project_path) = project_path.as_deref() {
                        match inspect_claude_plugin(
                            &claude_home,
                            std::path::Path::new(project_path),
                            &plugin_id,
                            enabled,
                        ) {
                            Ok(mut descriptor) => {
                                descriptor.declaration_path = location.clone();
                                content["classification"] = serde_json::to_value(
                                    classify_claude_plugin(Some(&descriptor), enabled),
                                )
                                .map_err(|error| {
                                    agent_error(
                                        AgentErrorCode::Io,
                                        context,
                                        None,
                                        error.to_string(),
                                    )
                                })?;
                                content["descriptor"] =
                                    serde_json::to_value(descriptor).map_err(|error| {
                                        agent_error(
                                            AgentErrorCode::Io,
                                            context,
                                            None,
                                            error.to_string(),
                                        )
                                    })?;
                            }
                            Err(error) => {
                                content["inspectionError"] =
                                    serde_json::Value::String(error.to_string());
                            }
                        }
                    }
                }
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
        _request: CollectionInstallRequest,
    ) -> Result<MutationPlan, AgentError> {
        Err(agent_error(
            AgentErrorCode::Unsupported,
            context,
            None,
            "Claude plugin installation is not managed by AD",
        ))
    }

    fn plan_set_enabled(
        &self,
        context: &AgentContext,
        resource: &ResourceRef,
        enabled: bool,
    ) -> Result<MutationPlan, AgentError> {
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
        plugins.insert(
            resource.logical_id.clone(),
            serde_json::Value::Bool(enabled),
        );
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
