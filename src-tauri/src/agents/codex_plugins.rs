use std::collections::BTreeSet;

use chrono::{Duration, Utc};

use super::codex_ports::{agent_error, read_optional, resolve_codex_home};
use super::{
    AgentContext, AgentError, AgentErrorCode, AgentId, CapabilityAvailability,
    CapabilityLimitation, CapabilityOperation, CollectionInstallRequest, ContentDigest,
    ManagedResourceTarget, MutationKind, MutationPlan, PlanId, PlannedMutation, PluginsPort,
    ReadPrecondition, ResourceKind, ResourceLocation, ResourceOrigin, ResourcePort, ResourceRef,
    ResourceScope, ResourceSnapshot, WritePolicy,
};

#[derive(Debug, Default)]
pub(crate) struct CodexPluginsPort;

impl ResourcePort for CodexPluginsPort {
    fn resolve(
        &self,
        context: &AgentContext,
        resource: &ResourceRef,
    ) -> Result<ManagedResourceTarget, AgentError> {
        validate_resource(context, resource)?;
        Ok(ManagedResourceTarget::file(
            resolve_codex_home(context)?.join("config.toml"),
        ))
    }
}

impl PluginsPort for CodexPluginsPort {
    fn scopes(&self) -> BTreeSet<ResourceScope> {
        BTreeSet::from([ResourceScope::User])
    }

    fn operations(&self) -> BTreeSet<CapabilityOperation> {
        BTreeSet::from([
            CapabilityOperation::List,
            CapabilityOperation::Install,
            CapabilityOperation::Enable,
            CapabilityOperation::Disable,
            CapabilityOperation::Preview,
            CapabilityOperation::Apply,
        ])
    }

    fn availability(&self) -> CapabilityAvailability {
        CapabilityAvailability::Degraded
    }

    fn limitations(&self) -> Vec<CapabilityLimitation> {
        vec![CapabilityLimitation {
            code: "install_requires_codex_marketplace_flow".into(),
            message_key: "agents.capabilities.codexPluginInstallRequiresMarketplace".into(),
        }]
    }

    fn list(&self, context: &AgentContext) -> Result<Vec<ResourceSnapshot>, AgentError> {
        let config = resolve_codex_home(context)?.join("config.toml");
        let Some(bytes) = read_optional(&config, context, None)? else {
            return Ok(Vec::new());
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
        let Some(plugins) = value.get("plugins").and_then(toml::Value::as_table) else {
            return Ok(Vec::new());
        };
        plugins
            .iter()
            .map(|(id, config_value)| {
                let enabled = config_value
                    .get("enabled")
                    .and_then(toml::Value::as_bool)
                    .unwrap_or(true);
                let content = serde_json::json!({"enabled": enabled});
                let digest_bytes = serde_json::to_vec(&content).map_err(|error| {
                    agent_error(AgentErrorCode::Io, context, None, error.to_string())
                })?;
                Ok(ResourceSnapshot {
                    resource: ResourceRef {
                        installation_id: context.installation_id.clone(),
                        project_path: None,
                        kind: ResourceKind::Plugins,
                        scope: ResourceScope::User,
                        logical_id: id.clone(),
                    },
                    location: ResourceLocation {
                        path: config.to_string_lossy().into_owned(),
                        origin: ResourceOrigin::User,
                    },
                    media_type: "application/vnd.ad.plugin+json".into(),
                    content,
                    digest: ContentDigest::sha256(&digest_bytes),
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
            "Codex plugin installation requires its marketplace and authorization flow",
        ))
    }

    fn plan_set_enabled(
        &self,
        context: &AgentContext,
        resource: &ResourceRef,
        enabled: bool,
    ) -> Result<MutationPlan, AgentError> {
        validate_resource(context, resource)?;
        let config = resolve_codex_home(context)?.join("config.toml");
        let existing = read_optional(&config, context, Some(resource.clone()))?;
        let mut value = match existing.as_deref() {
            Some(bytes) => std::str::from_utf8(bytes)
                .map_err(|error| {
                    agent_error(
                        AgentErrorCode::InvalidPlan,
                        context,
                        Some(resource.clone()),
                        error.to_string(),
                    )
                })?
                .parse::<toml::Value>()
                .map_err(|error| {
                    agent_error(
                        AgentErrorCode::InvalidPlan,
                        context,
                        Some(resource.clone()),
                        error.to_string(),
                    )
                })?,
            None => toml::Value::Table(toml::map::Map::new()),
        };
        let root = value.as_table_mut().ok_or_else(|| {
            agent_error(
                AgentErrorCode::InvalidPlan,
                context,
                Some(resource.clone()),
                "Codex config must be a TOML table",
            )
        })?;
        let plugins = root
            .entry("plugins")
            .or_insert_with(|| toml::Value::Table(toml::map::Map::new()))
            .as_table_mut()
            .ok_or_else(|| {
                agent_error(
                    AgentErrorCode::InvalidPlan,
                    context,
                    Some(resource.clone()),
                    "plugins must be a TOML table",
                )
            })?;
        let plugin = plugins
            .entry(resource.logical_id.clone())
            .or_insert_with(|| toml::Value::Table(toml::map::Map::new()))
            .as_table_mut()
            .ok_or_else(|| {
                agent_error(
                    AgentErrorCode::InvalidPlan,
                    context,
                    Some(resource.clone()),
                    "plugin config must be a TOML table",
                )
            })?;
        plugin.insert("enabled".into(), toml::Value::Boolean(enabled));
        let rendered = toml::to_string_pretty(&value).map_err(|error| {
            agent_error(
                AgentErrorCode::InvalidPlan,
                context,
                Some(resource.clone()),
                error.to_string(),
            )
        })?;
        let expected_digest = existing.as_deref().map(ContentDigest::sha256);
        Ok(MutationPlan {
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
                resource: resource.clone(),
                kind: if expected_digest.is_some() {
                    MutationKind::Replace
                } else {
                    MutationKind::Create
                },
                expected_digest,
                media_type: "application/toml".into(),
                content: Some(serde_json::Value::String(rendered)),
            }],
            expires_at: Utc::now() + Duration::minutes(5),
        })
    }
}

fn validate_resource(context: &AgentContext, resource: &ResourceRef) -> Result<(), AgentError> {
    if resource.installation_id != context.installation_id
        || resource.kind != ResourceKind::Plugins
        || resource.scope != ResourceScope::User
        || resource.project_path.is_some()
        || resource.logical_id.is_empty()
    {
        return Err(agent_error(
            AgentErrorCode::InvalidPlan,
            context,
            Some(resource.clone()),
            "Plugin resource does not belong to the active Agent context",
        ));
    }
    Ok(())
}
