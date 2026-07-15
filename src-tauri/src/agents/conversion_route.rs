use std::collections::BTreeMap;

use chrono::{Duration, Utc};
use serde_json::Value;

use super::conversion::{map_claude_setting, ArtifactDisposition, ConversionArtifact};
use super::{
    builtin_registry, AdapterRegistry, AgentAdapter, AgentContext, AgentError, AgentErrorCode,
    AgentId, ContentDigest, MutationPlan, PlanId, ReadPrecondition, ResourceKind, ResourceRef,
    ResourceScope, ResourceSnapshot, SettingsEdit, WritePolicy,
};

/// Backend-owned conversion result. The mutation content remains private until
/// Task 16 stores the plan and returns only a public plan view.
#[derive(Debug, Clone, PartialEq)]
pub struct ConversionRoutePlan {
    pub source_agent_id: AgentId,
    pub target_agent_id: AgentId,
    pub artifacts: Vec<ConversionArtifact>,
    pub plan: MutationPlan,
}

pub trait ConversionRoute {
    fn source_agent_id(&self) -> &'static str;
    fn target_agent_id(&self) -> &'static str;

    fn preview(
        &self,
        source_context: &AgentContext,
        target_context: &AgentContext,
    ) -> Result<ConversionRoutePlan, AgentError>;
}

#[derive(Debug, Default)]
pub struct ClaudeToCodexRoute;

impl ConversionRoute for ClaudeToCodexRoute {
    fn source_agent_id(&self) -> &'static str {
        "claude-code"
    }

    fn target_agent_id(&self) -> &'static str {
        "codex"
    }

    fn preview(
        &self,
        source_context: &AgentContext,
        target_context: &AgentContext,
    ) -> Result<ConversionRoutePlan, AgentError> {
        let registry = builtin_registry();
        let source_adapter =
            adapter_for_context(&registry, source_context, self.source_agent_id(), "source")?;
        let target_adapter =
            adapter_for_context(&registry, target_context, self.target_agent_id(), "target")?;
        if source_context.project_path != target_context.project_path {
            return Err(route_error(
                target_context,
                "Source and target project contexts must match",
            ));
        }
        let source_settings = source_adapter
            .settings()
            .ok_or_else(|| route_error(source_context, "Source Agent does not expose settings"))?;
        let target_settings = target_adapter
            .settings()
            .ok_or_else(|| route_error(target_context, "Target Agent does not expose settings"))?;
        let sources = source_settings.inspect(source_context)?;
        let targets = target_settings.inspect(target_context)?;

        build_settings_route(
            source_context,
            target_context,
            target_settings,
            sources,
            targets,
        )
    }
}

fn adapter_for_context<'a>(
    registry: &'a AdapterRegistry,
    context: &AgentContext,
    expected_agent_id: &str,
    endpoint: &str,
) -> Result<&'a dyn AgentAdapter, AgentError> {
    let installation = registry
        .discover()
        .into_iter()
        .find(|installation| installation.id == context.installation_id)
        .ok_or_else(|| route_error(context, format!("Unknown {endpoint} Agent installation")))?;
    if installation.agent_id.as_str() != expected_agent_id {
        return Err(route_error(
            context,
            format!(
                "Conversion {endpoint} must be {expected_agent_id}, found {}",
                installation.agent_id
            ),
        ));
    }
    registry
        .adapter(expected_agent_id)
        .ok_or_else(|| route_error(context, format!("Unknown {endpoint} Agent adapter")))
}

fn build_settings_route(
    source_context: &AgentContext,
    target_context: &AgentContext,
    target_settings: &dyn super::SettingsPort,
    sources: Vec<ResourceSnapshot>,
    targets: Vec<ResourceSnapshot>,
) -> Result<ConversionRoutePlan, AgentError> {
    let mut source_groups = BTreeMap::<ResourceScope, Vec<ResourceSnapshot>>::new();
    for snapshot in sources {
        validate_snapshot_context(&snapshot, source_context, "source")?;
        if snapshot.resource.kind != ResourceKind::Settings
            || snapshot.media_type != "application/json"
        {
            return Err(route_error(
                source_context,
                "Claude conversion source must contain JSON settings snapshots",
            ));
        }
        source_groups
            .entry(snapshot.resource.scope)
            .or_default()
            .push(snapshot);
    }

    let mut target_by_scope = BTreeMap::new();
    for snapshot in targets {
        validate_snapshot_context(&snapshot, target_context, "target")?;
        if snapshot.resource.kind != ResourceKind::Settings
            || snapshot.media_type != "application/toml"
        {
            return Err(route_error(
                target_context,
                "Codex conversion target must contain TOML settings snapshots",
            ));
        }
        if target_by_scope
            .insert(snapshot.resource.scope, snapshot)
            .is_some()
        {
            return Err(route_error(
                target_context,
                "Codex settings inspection returned duplicate scopes",
            ));
        }
    }

    let mut artifacts = Vec::new();
    let mut read_set = Vec::new();
    let mut mutations = Vec::new();
    for (scope, mut scope_sources) in source_groups {
        scope_sources.sort_by_key(|snapshot| source_layer_order(&snapshot.resource.logical_id));
        let effective = effective_source_fields(source_context, &scope_sources)?;
        let target_snapshot = target_by_scope.remove(&scope);
        let target_resource = target_snapshot
            .as_ref()
            .map(|snapshot| snapshot.resource.clone())
            .unwrap_or_else(|| target_resource(target_context, scope));
        let original = target_table(target_context, target_snapshot.as_ref())?;
        let mut merged = original.clone();

        for (field, (source, value)) in effective {
            let Some(mapping) = map_claude_setting(&field, &value) else {
                continue;
            };
            let id = format!("{}:{field}", source.logical_id);
            let mut disposition = mapping.disposition;
            let mut message = mapping.message;
            let target = mapping.target_key.as_ref().map(|_| target_resource.clone());

            if let (Some(target_key), Some(target_value)) =
                (mapping.target_key.as_ref(), mapping.target_value.as_ref())
            {
                match original.get(target_key) {
                    Some(existing) if existing == target_value => {
                        disposition = ArtifactDisposition::Unchanged;
                        message =
                            format!("Target already has an equivalent value for {target_key}");
                    }
                    Some(_) => {
                        disposition = ArtifactDisposition::Conflict;
                        message = format!(
                            "Target already defines {target_key}; existing value is preserved"
                        );
                    }
                    None => {
                        merged.insert(target_key.clone(), target_value.clone());
                    }
                }
            }

            artifacts.push(ConversionArtifact {
                id,
                kind: mapping.kind,
                source,
                target,
                disposition,
                message,
            });
        }

        if merged != original {
            let content = toml::to_string_pretty(&toml::Value::Table(merged)).map_err(|error| {
                route_error(
                    target_context,
                    format!("Failed to render merged Codex config: {error}"),
                )
            })?;
            let target_plan = target_settings.plan_edit(
                target_context,
                SettingsEdit {
                    resource: target_resource,
                    media_type: "application/toml".into(),
                    content: Value::String(content),
                },
            )?;
            read_set.extend(scope_sources.iter().map(|snapshot| ReadPrecondition {
                resource: snapshot.resource.clone(),
                expected_digest: snapshot.digest.clone(),
                write_policy: WritePolicy::ReadOnly,
            }));
            read_set.extend(target_plan.read_set);
            mutations.extend(target_plan.mutations);
        }
    }

    let plan = MutationPlan {
        id: PlanId::from(uuid::Uuid::new_v4().to_string()),
        agent_id: AgentId::from("codex"),
        context: target_context.clone(),
        read_set: deduplicate_preconditions(read_set)?,
        mutations,
        expires_at: Utc::now() + Duration::minutes(5),
    };
    let result = ConversionRoutePlan {
        source_agent_id: AgentId::from("claude-code"),
        target_agent_id: AgentId::from("codex"),
        artifacts,
        plan,
    };
    validate_route_plan(&result, source_context, target_context)?;
    Ok(result)
}

fn validate_snapshot_context(
    snapshot: &ResourceSnapshot,
    context: &AgentContext,
    endpoint: &str,
) -> Result<(), AgentError> {
    if snapshot.resource.installation_id != context.installation_id
        || snapshot.resource.project_path != context.project_path
            && snapshot.resource.scope == ResourceScope::Project
    {
        return Err(route_error(
            context,
            format!("Conversion {endpoint} snapshot does not belong to its context"),
        ));
    }
    Ok(())
}

fn validate_route_plan(
    result: &ConversionRoutePlan,
    source_context: &AgentContext,
    target_context: &AgentContext,
) -> Result<(), AgentError> {
    result.plan.validate()?;
    if result.plan.agent_id != result.target_agent_id
        || result.plan.context != *target_context
        || result.source_agent_id.as_str() != "claude-code"
        || result.target_agent_id.as_str() != "codex"
    {
        return Err(route_error(
            target_context,
            "Conversion plan endpoint identity is inconsistent",
        ));
    }
    if result.plan.mutations.iter().any(|mutation| {
        mutation.resource.installation_id != target_context.installation_id
            || mutation.resource.installation_id == source_context.installation_id
    }) {
        return Err(route_error(
            target_context,
            "Conversion write-set must contain target resources only",
        ));
    }
    if result.plan.read_set.iter().any(|precondition| {
        precondition.resource.installation_id == source_context.installation_id
            && precondition.write_policy != WritePolicy::ReadOnly
    }) {
        return Err(route_error(
            target_context,
            "Conversion source resources must be read-only",
        ));
    }
    Ok(())
}

fn effective_source_fields(
    context: &AgentContext,
    snapshots: &[ResourceSnapshot],
) -> Result<BTreeMap<String, (ResourceRef, Value)>, AgentError> {
    let mut fields = BTreeMap::new();
    for snapshot in snapshots {
        let object = snapshot.content.as_object().ok_or_else(|| {
            route_error(context, "Claude settings snapshot must be a JSON object")
        })?;
        for (field, value) in object {
            fields.insert(field.clone(), (snapshot.resource.clone(), value.clone()));
        }
    }
    Ok(fields)
}

fn source_layer_order(logical_id: &str) -> u8 {
    match logical_id {
        "user-settings" | "project-shared" => 0,
        "project-local" => 1,
        _ => 2,
    }
}

fn target_resource(context: &AgentContext, scope: ResourceScope) -> ResourceRef {
    ResourceRef {
        installation_id: context.installation_id.clone(),
        project_path: (scope == ResourceScope::Project)
            .then(|| context.project_path.clone())
            .flatten(),
        kind: ResourceKind::Settings,
        scope,
        logical_id: match scope {
            ResourceScope::User => "user-config",
            ResourceScope::Project => "project-config",
        }
        .into(),
    }
}

fn target_table(
    context: &AgentContext,
    snapshot: Option<&ResourceSnapshot>,
) -> Result<toml::map::Map<String, toml::Value>, AgentError> {
    let Some(snapshot) = snapshot else {
        return Ok(toml::map::Map::new());
    };
    let content = snapshot
        .content
        .as_str()
        .ok_or_else(|| route_error(context, "Codex settings snapshot must contain TOML text"))?;
    let value = content
        .parse::<toml::Value>()
        .map_err(|error| route_error(context, format!("Invalid Codex target TOML: {error}")))?;
    value
        .as_table()
        .cloned()
        .ok_or_else(|| route_error(context, "Codex config root must be a table"))
}

fn deduplicate_preconditions(
    preconditions: Vec<ReadPrecondition>,
) -> Result<Vec<ReadPrecondition>, AgentError> {
    let mut unique = BTreeMap::<ResourceRef, (ContentDigest, WritePolicy)>::new();
    for precondition in preconditions {
        if let Some((digest, policy)) = unique.get(&precondition.resource) {
            if digest != &precondition.expected_digest || policy != &precondition.write_policy {
                return Err(AgentError {
                    code: AgentErrorCode::InvalidPlan,
                    message: format!(
                        "Conversion produced inconsistent preconditions for {}",
                        precondition.resource.logical_id
                    ),
                    agent_id: Some(AgentId::from("codex")),
                    installation_id: Some(precondition.resource.installation_id.clone()),
                    resource: Some(precondition.resource),
                    retryable: false,
                    details: None,
                });
            }
            continue;
        }
        unique.insert(
            precondition.resource,
            (precondition.expected_digest, precondition.write_policy),
        );
    }
    Ok(unique
        .into_iter()
        .map(
            |(resource, (expected_digest, write_policy))| ReadPrecondition {
                resource,
                expected_digest,
                write_policy,
            },
        )
        .collect())
}

fn route_error(context: &AgentContext, message: impl Into<String>) -> AgentError {
    AgentError {
        code: AgentErrorCode::InvalidPlan,
        message: message.into(),
        agent_id: None,
        installation_id: Some(context.installation_id.clone()),
        resource: None,
        retryable: false,
        details: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::{InstallationId, MutationKind, PlannedMutation};

    fn context(installation_id: &str) -> AgentContext {
        AgentContext {
            installation_id: InstallationId::from(installation_id),
            project_path: None,
        }
    }

    fn resource(installation_id: &str) -> ResourceRef {
        ResourceRef {
            installation_id: InstallationId::from(installation_id),
            project_path: None,
            kind: ResourceKind::Settings,
            scope: ResourceScope::User,
            logical_id: "user-config".into(),
        }
    }

    #[test]
    fn conversion_invariant_rejects_source_resources_in_write_set() {
        let source = context("claude-code:source");
        let target = context("codex:target");
        let result = ConversionRoutePlan {
            source_agent_id: AgentId::from("claude-code"),
            target_agent_id: AgentId::from("codex"),
            artifacts: Vec::new(),
            plan: MutationPlan {
                id: PlanId::from("plan"),
                agent_id: AgentId::from("codex"),
                context: target.clone(),
                read_set: Vec::new(),
                mutations: vec![PlannedMutation {
                    resource: resource("claude-code:source"),
                    kind: MutationKind::Replace,
                    expected_digest: None,
                    media_type: "application/json".into(),
                    content: Some(serde_json::json!({})),
                }],
                expires_at: Utc::now() + Duration::minutes(5),
            },
        };

        let error = validate_route_plan(&result, &source, &target).unwrap_err();

        assert!(error.message.contains("target resources only"));
    }

    #[test]
    fn non_string_model_requires_user_input() {
        let mapping = map_claude_setting("model", &serde_json::json!({"name": "gpt-5.4"})).unwrap();

        assert_eq!(mapping.disposition, ArtifactDisposition::RequiresInput);
        assert!(mapping.target_value.is_none());
    }
}
