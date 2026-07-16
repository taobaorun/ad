use std::collections::{BTreeMap, BTreeSet};

use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::conversion::{
    explicit_targets, map_claude_setting, map_plugin_artifact, map_skill_artifact,
    ArtifactDisposition, ConversionArtifact, ConversionEndpoint, ConversionRiskLevel,
    ConversionSummary, FieldMapping,
};
use super::execution_fs::observe_target;
use super::{
    builtin_registry, AdapterRegistry, AgentAdapter, AgentContext, AgentError, AgentErrorCode,
    AgentId, CollectionInstallRequest, ContentDigest, MutationPlan, MutationPlanView, PlanId,
    ReadPrecondition, ResourceKind, ResourceLocation, ResourceOrigin, ResourceRef, ResourceScope,
    ResourceSnapshot, SettingsEdit, WritePolicy,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversionRoutePreview {
    pub source_agent_id: AgentId,
    pub target_agent_id: AgentId,
    pub artifacts: Vec<ConversionArtifact>,
    pub summary: ConversionSummary,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan: Option<MutationPlanView>,
}

/// Backend-owned conversion result. The mutation content remains private until
/// Task 16 stores the plan and returns only a public plan view.
#[derive(Debug, Clone, PartialEq)]
pub struct ConversionRoutePlan {
    pub source_agent_id: AgentId,
    pub target_agent_id: AgentId,
    pub artifacts: Vec<ConversionArtifact>,
    pub summary: ConversionSummary,
    pub plan: MutationPlan,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ClaudeToCodexOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permission_preset: Option<CodexPermissionPreset>,
    #[serde(default)]
    pub confirmed_skill_ids: BTreeSet<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CodexPermissionPreset {
    OnRequestWorkspaceWrite,
    NeverDangerFullAccess,
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
        self.preview_with_options(
            source_context,
            target_context,
            &ClaudeToCodexOptions::default(),
        )
    }
}

impl ClaudeToCodexRoute {
    pub fn preview_with_options(
        &self,
        source_context: &AgentContext,
        target_context: &AgentContext,
        options: &ClaudeToCodexOptions,
    ) -> Result<ConversionRoutePlan, AgentError> {
        validate_options(target_context, options)?;
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
        let scope = conversion_scope(source_context);
        let source_settings = source_adapter
            .settings()
            .ok_or_else(|| route_error(source_context, "Source Agent does not expose settings"))?;
        let target_settings = target_adapter
            .settings()
            .ok_or_else(|| route_error(target_context, "Target Agent does not expose settings"))?;
        let mut result = build_settings_route(
            source_context,
            target_context,
            target_settings,
            snapshots_in_scope(source_settings.inspect(source_context)?, scope),
            snapshots_in_scope(target_settings.inspect(target_context)?, scope),
            options,
        )?;
        append_collection_artifacts(
            source_adapter,
            target_adapter,
            source_context,
            target_context,
            scope,
            options,
            &mut result,
        )?;
        result.summary = ConversionSummary::from_artifacts(&result.artifacts);
        validate_route_plan(&result, source_context, target_context)?;
        Ok(result)
    }
}

fn validate_options(
    context: &AgentContext,
    options: &ClaudeToCodexOptions,
) -> Result<(), AgentError> {
    if let Some(model) = &options.target_model {
        let trimmed = model.trim();
        if trimmed.is_empty()
            || trimmed.len() > 200
            || trimmed.chars().any(char::is_control)
            || trimmed != model
        {
            return Err(route_error(
                context,
                "Codex target model must be a non-empty model id without surrounding whitespace",
            ));
        }
    }
    Ok(())
}

fn append_collection_artifacts(
    source_adapter: &dyn AgentAdapter,
    target_adapter: &dyn AgentAdapter,
    source_context: &AgentContext,
    target_context: &AgentContext,
    scope: ResourceScope,
    options: &ClaudeToCodexOptions,
    result: &mut ConversionRoutePlan,
) -> Result<(), AgentError> {
    if let (Some(source_port), Some(target_port)) =
        (source_adapter.skills(), target_adapter.skills())
    {
        let targets = snapshots_in_scope(target_port.list(target_context)?, scope);
        for source in snapshots_in_scope(source_port.list(source_context)?, scope) {
            let name = source.content.get("name").and_then(Value::as_str);
            let target = name.and_then(|name| {
                targets.iter().find(|target| {
                    target.resource.scope == source.resource.scope
                        && target.resource.logical_id == name
                })
            });
            let Some(name) = name else {
                continue;
            };
            let target_resource = target
                .map(|snapshot| snapshot.resource.clone())
                .unwrap_or_else(|| {
                    collection_resource(target_context, ResourceKind::Skills, scope, name)
                });
            let target_location = target_port
                .resolve(target_context, &target_resource)?
                .path()
                .to_string_lossy()
                .into_owned();
            let confirmed = options.confirmed_skill_ids.contains(name);
            if let Some(artifact) = map_skill_artifact(
                &source,
                target_context,
                target,
                ResourceLocation {
                    path: target_location,
                    origin: scope_origin(scope),
                },
                confirmed,
            ) {
                if target.is_none() && confirmed {
                    let source_digest =
                        observe_target(&source_port.resolve(source_context, &source.resource)?)?
                            .digest()
                            .ok_or_else(|| {
                                route_error(
                                    source_context,
                                    "Confirmed Skill source no longer exists",
                                )
                            })?;
                    let install = target_port.plan_install(
                        target_context,
                        CollectionInstallRequest {
                            logical_id: name.into(),
                            source: serde_json::json!({"path": source.location.path}),
                        },
                    )?;
                    result.plan.read_set.push(ReadPrecondition {
                        resource: source.resource.clone(),
                        expected_digest: source_digest,
                        write_policy: WritePolicy::ReadOnly,
                    });
                    result.plan.read_set.extend(install.read_set);
                    result.plan.mutations.extend(install.mutations);
                }
                result.artifacts.push(artifact);
            }
        }
    }
    if let (Some(source_port), Some(target_port)) =
        (source_adapter.plugins(), target_adapter.plugins())
    {
        let targets = snapshots_in_scope(target_port.list(target_context)?, scope);
        for source in snapshots_in_scope(source_port.list(source_context)?, scope) {
            let target = targets.iter().find(|target| {
                target.resource.logical_id == source.resource.logical_id
                    && target.resource.scope == source.resource.scope
            });
            let target_resource = target
                .map(|snapshot| snapshot.resource.clone())
                .or_else(|| {
                    (scope == ResourceScope::User).then(|| {
                        collection_resource(
                            target_context,
                            ResourceKind::Plugins,
                            scope,
                            &source.resource.logical_id,
                        )
                    })
                });
            let target_location = target_resource.as_ref().and_then(|resource| {
                target_port
                    .resolve(target_context, resource)
                    .ok()
                    .map(|resolved| ResourceLocation {
                        path: resolved.path().to_string_lossy().into_owned(),
                        origin: scope_origin(scope),
                    })
            });
            result.artifacts.push(map_plugin_artifact(
                &source,
                target_context,
                target,
                target_location,
            ));
        }
    }
    result
        .artifacts
        .sort_by(|left, right| left.id.cmp(&right.id));
    result.plan.read_set = deduplicate_preconditions(std::mem::take(&mut result.plan.read_set))?;
    Ok(())
}

fn collection_resource(
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

fn scope_origin(scope: ResourceScope) -> ResourceOrigin {
    match scope {
        ResourceScope::User => ResourceOrigin::User,
        ResourceScope::Project => ResourceOrigin::Project,
    }
}

fn conversion_scope(context: &AgentContext) -> ResourceScope {
    if context.project_path.is_some() {
        ResourceScope::Project
    } else {
        ResourceScope::User
    }
}

fn snapshots_in_scope(
    snapshots: Vec<ResourceSnapshot>,
    scope: ResourceScope,
) -> Vec<ResourceSnapshot> {
    snapshots
        .into_iter()
        .filter(|snapshot| snapshot.resource.scope == scope)
        .collect()
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
    options: &ClaudeToCodexOptions,
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
        let target_location = target_snapshot
            .as_ref()
            .map(|snapshot| snapshot.location.clone())
            .unwrap_or(ResourceLocation {
                path: target_settings
                    .resolve(target_context, &target_resource)?
                    .path()
                    .to_string_lossy()
                    .into_owned(),
                origin: scope_origin(scope),
            });
        let original = target_table(target_context, target_snapshot.as_ref())?;
        let mut merged = original.clone();

        for (field, (source, value)) in effective {
            if field == "enabledPlugins" {
                continue;
            }
            if field == "extraKnownMarketplaces" {
                append_marketplace_artifacts(&mut artifacts, &source, &value);
                continue;
            }
            let permission_rule_count = (field == "permissions")
                .then(|| count_permission_rules(&value))
                .unwrap_or(0);
            let Some(mapping) = resolved_setting_mapping(&field, &value, options) else {
                continue;
            };
            let id = format!("{}:{field}", source.resource.logical_id);
            let mut disposition = mapping.disposition;
            let mut message = mapping.message;
            let target = (!mapping.target_values.is_empty()).then(|| ConversionEndpoint {
                resource: target_resource.clone(),
                location: target_location.clone(),
            });

            if !mapping.target_values.is_empty() {
                let mut changed = false;
                let mut conflicts = Vec::new();
                for (target_key, target_value) in &mapping.target_values {
                    match original.get(target_key) {
                        Some(existing) if existing == target_value => {}
                        Some(_) if mapping.replace_existing => {
                            merged.insert(target_key.clone(), target_value.clone());
                            changed = true;
                        }
                        Some(_) => conflicts.push(target_key.as_str()),
                        None => {
                            merged.insert(target_key.clone(), target_value.clone());
                            changed = true;
                        }
                    }
                }
                if !conflicts.is_empty() {
                    disposition = ArtifactDisposition::Conflict;
                    message = format!(
                        "Target already defines {}; existing value is preserved",
                        conflicts.join(", ")
                    );
                } else if !changed {
                    disposition = ArtifactDisposition::Unchanged;
                    message = "Target already has equivalent values".into();
                }
            }

            artifacts.push(ConversionArtifact {
                id,
                kind: mapping.kind,
                source: source.clone(),
                target,
                disposition,
                resolution: mapping.resolution,
                risk: mapping.risk,
                message,
            });
            if permission_rule_count > 0 {
                artifacts.push(ConversionArtifact {
                    id: format!("{}:permissions:rules", source.resource.logical_id),
                    kind: ResourceKind::Rules,
                    source: source.clone(),
                    target: None,
                    disposition: ArtifactDisposition::Unsupported,
                    resolution: None,
                    risk: ConversionRiskLevel::Safe,
                    message: format!(
                        "{permission_rule_count} fine-grained Claude permission rules have no lossless Codex mapping"
                    ),
                });
            }
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
        summary: ConversionSummary::default(),
        plan,
    };
    validate_route_plan(&result, source_context, target_context)?;
    Ok(result)
}

fn append_marketplace_artifacts(
    artifacts: &mut Vec<ConversionArtifact>,
    source: &ConversionEndpoint,
    value: &Value,
) {
    let Some(marketplaces) = value.as_object() else {
        return;
    };
    for marketplace_id in marketplaces.keys() {
        artifacts.push(ConversionArtifact {
            id: format!("marketplace:{marketplace_id}"),
            kind: ResourceKind::Plugins,
            source: source.clone(),
            target: None,
            disposition: ArtifactDisposition::Unsupported,
            resolution: None,
            risk: ConversionRiskLevel::Confirmation,
            message: format!(
                "Marketplace {marketplace_id} must be configured through the Codex plugin marketplace"
            ),
        });
    }
}

fn count_permission_rules(value: &Value) -> usize {
    ["allow", "ask", "deny"]
        .into_iter()
        .filter_map(|key| value.get(key).and_then(Value::as_array))
        .map(Vec::len)
        .sum()
}

fn resolved_setting_mapping(
    field: &str,
    value: &Value,
    options: &ClaudeToCodexOptions,
) -> Option<FieldMapping> {
    let default_mapping = map_claude_setting(field, value)?;
    match field {
        "model" => options
            .target_model
            .as_ref()
            .map(|model| {
                explicit_targets(
                    ResourceKind::Settings,
                    [("model".into(), toml::Value::String(model.clone()))],
                    "User-selected Codex model replaces the target model",
                )
            })
            .or(Some(default_mapping)),
        "permissions" => options
            .permission_preset
            .map(|preset| {
                let (approval_policy, sandbox_mode, message) = match preset {
                    CodexPermissionPreset::OnRequestWorkspaceWrite => (
                        "on-request",
                        "workspace-write",
                        "User selected interactive approval with workspace-write sandboxing",
                    ),
                    CodexPermissionPreset::NeverDangerFullAccess => (
                        "never",
                        "danger-full-access",
                        "User explicitly selected the bypass-permissions equivalent",
                    ),
                };
                let mut mapping = explicit_targets(
                    ResourceKind::Settings,
                    [
                        (
                            "approval_policy".into(),
                            toml::Value::String(approval_policy.into()),
                        ),
                        (
                            "sandbox_mode".into(),
                            toml::Value::String(sandbox_mode.into()),
                        ),
                    ],
                    message,
                );
                if preset == CodexPermissionPreset::NeverDangerFullAccess {
                    mapping.risk = ConversionRiskLevel::Dangerous;
                }
                mapping
            })
            .or(Some(default_mapping)),
        _ => Some(default_mapping),
    }
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
) -> Result<BTreeMap<String, (ConversionEndpoint, Value)>, AgentError> {
    let mut fields = BTreeMap::new();
    for snapshot in snapshots {
        let object = snapshot.content.as_object().ok_or_else(|| {
            route_error(context, "Claude settings snapshot must be a JSON object")
        })?;
        for (field, value) in object {
            fields.insert(
                field.clone(),
                (ConversionEndpoint::from(snapshot), value.clone()),
            );
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
            summary: ConversionSummary::default(),
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
        assert!(mapping.target_values.is_empty());
    }

    #[test]
    fn claude_model_name_requires_an_explicit_codex_model() {
        let mapping = map_claude_setting("model", &serde_json::json!("opus[1m]")).unwrap();

        assert_eq!(mapping.disposition, ArtifactDisposition::RequiresInput);
        assert!(mapping.target_values.is_empty());
    }

    #[test]
    fn max_context_tokens_maps_to_codex_context_window() {
        let mapping = map_claude_setting("maxContextTokens", &serde_json::json!(250_000)).unwrap();

        assert_eq!(mapping.disposition, ArtifactDisposition::Mapped);
        assert_eq!(
            mapping.target_values.get("model_context_window"),
            Some(&toml::Value::Integer(250_000))
        );
    }
}
