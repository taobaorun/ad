use std::collections::BTreeSet;
use std::io::ErrorKind;
use std::path::PathBuf;

use chrono::{Duration, Utc};

use super::codex::discover_codex_candidates;
use super::{
    AgentContext, AgentError, AgentErrorCode, AgentId, CapabilityAvailability, CapabilityOperation,
    ContentDigest, ManagedResourceTarget, MutationKind, MutationPlan, PlanId, PlannedMutation,
    ReadPrecondition, ResourceKind, ResourceLocation, ResourceOrigin, ResourcePort, ResourceRef,
    ResourceScope, ResourceSnapshot, SettingsEdit, SettingsPort, WritePolicy,
};

#[derive(Debug, Default)]
pub(crate) struct CodexSettingsPort;

impl ResourcePort for CodexSettingsPort {
    fn resolve(
        &self,
        context: &AgentContext,
        resource: &ResourceRef,
    ) -> Result<ManagedResourceTarget, AgentError> {
        Ok(ManagedResourceTarget::file(resolve_settings_path(
            context, resource,
        )?))
    }
}

impl SettingsPort for CodexSettingsPort {
    fn scopes(&self) -> BTreeSet<ResourceScope> {
        BTreeSet::from([ResourceScope::User, ResourceScope::Project])
    }

    fn operations(&self) -> BTreeSet<CapabilityOperation> {
        BTreeSet::from([
            CapabilityOperation::Inspect,
            CapabilityOperation::Edit,
            CapabilityOperation::Preview,
            CapabilityOperation::Apply,
            CapabilityOperation::Rollback,
        ])
    }

    fn availability(&self) -> CapabilityAvailability {
        CapabilityAvailability::Available
    }

    fn inspect(&self, context: &AgentContext) -> Result<Vec<ResourceSnapshot>, AgentError> {
        let mut snapshots = Vec::new();
        push_snapshot(
            &mut snapshots,
            context,
            ResourceScope::User,
            "user-config",
            resolve_codex_home(context)?.join("config.toml"),
            ResourceOrigin::User,
        )?;
        if let Some(project_path) = &context.project_path {
            push_snapshot(
                &mut snapshots,
                context,
                ResourceScope::Project,
                "project-config",
                validate_project_path(context, project_path)?.join(".codex/config.toml"),
                ResourceOrigin::Project,
            )?;
        }
        Ok(snapshots)
    }

    fn plan_edit(
        &self,
        context: &AgentContext,
        edit: SettingsEdit,
    ) -> Result<MutationPlan, AgentError> {
        if edit.media_type != "application/toml" {
            return Err(agent_error(
                AgentErrorCode::InvalidPlan,
                context,
                Some(edit.resource),
                "Codex settings edits require application/toml",
            ));
        }
        let content = edit.content.as_str().ok_or_else(|| {
            agent_error(
                AgentErrorCode::InvalidPlan,
                context,
                Some(edit.resource.clone()),
                "Codex settings content must be TOML text",
            )
        })?;
        content.parse::<toml::Value>().map_err(|error| {
            agent_error(
                AgentErrorCode::InvalidPlan,
                context,
                Some(edit.resource.clone()),
                format!("Invalid Codex config TOML: {error}"),
            )
        })?;
        let path = resolve_settings_path(context, &edit.resource)?;
        let existing = read_optional(&path, context, Some(edit.resource.clone()))?;
        let expected_digest = existing.as_deref().map(ContentDigest::sha256);
        let plan = MutationPlan {
            id: PlanId::from(uuid::Uuid::new_v4().to_string()),
            agent_id: AgentId::from("codex"),
            context: context.clone(),
            read_set: expected_digest
                .clone()
                .map(|digest| {
                    vec![ReadPrecondition {
                        resource: edit.resource.clone(),
                        expected_digest: digest,
                        write_policy: WritePolicy::Mutable,
                    }]
                })
                .unwrap_or_default(),
            mutations: vec![PlannedMutation {
                resource: edit.resource,
                kind: if existing.is_some() {
                    MutationKind::Replace
                } else {
                    MutationKind::Create
                },
                expected_digest,
                media_type: edit.media_type,
                content: Some(edit.content),
            }],
            expires_at: Utc::now() + Duration::minutes(5),
        };
        plan.validate()?;
        Ok(plan)
    }
}

fn push_snapshot(
    snapshots: &mut Vec<ResourceSnapshot>,
    context: &AgentContext,
    scope: ResourceScope,
    logical_id: &str,
    path: PathBuf,
    origin: ResourceOrigin,
) -> Result<(), AgentError> {
    let resource = ResourceRef {
        installation_id: context.installation_id.clone(),
        project_path: (scope == ResourceScope::Project)
            .then(|| context.project_path.clone())
            .flatten(),
        kind: ResourceKind::Settings,
        scope,
        logical_id: logical_id.into(),
    };
    let Some(bytes) = read_optional(&path, context, Some(resource.clone()))? else {
        return Ok(());
    };
    let content = String::from_utf8(bytes.clone()).map_err(|error| {
        agent_error(
            AgentErrorCode::InvalidPlan,
            context,
            Some(resource.clone()),
            format!("Codex config is not UTF-8 at {}: {error}", path.display()),
        )
    })?;
    content.parse::<toml::Value>().map_err(|error| {
        agent_error(
            AgentErrorCode::InvalidPlan,
            context,
            Some(resource.clone()),
            format!("Invalid Codex config TOML at {}: {error}", path.display()),
        )
    })?;
    snapshots.push(ResourceSnapshot {
        resource,
        location: ResourceLocation {
            path: path.to_string_lossy().into_owned(),
            origin,
        },
        media_type: "application/toml".into(),
        content: serde_json::Value::String(content),
        digest: ContentDigest::sha256(&bytes),
        observed_at: Utc::now(),
    });
    Ok(())
}

fn resolve_codex_home(context: &AgentContext) -> Result<PathBuf, AgentError> {
    discover_codex_candidates()
        .into_iter()
        .find(|candidate| candidate.installation().id == context.installation_id)
        .map(|candidate| PathBuf::from(&candidate.installation().root_path))
        .ok_or_else(|| {
            agent_error(
                AgentErrorCode::InvalidPlan,
                context,
                None,
                "Agent context does not target a discovered Codex installation",
            )
        })
}

fn validate_project_path(
    context: &AgentContext,
    project_path: &str,
) -> Result<PathBuf, AgentError> {
    let canonical = std::fs::canonicalize(project_path).map_err(|error| {
        agent_error(
            AgentErrorCode::InvalidPlan,
            context,
            None,
            format!("Invalid project path {project_path}: {error}"),
        )
    })?;
    if !canonical.is_dir() || canonical.to_string_lossy() != project_path {
        return Err(agent_error(
            AgentErrorCode::InvalidPlan,
            context,
            None,
            "Agent context project path is not canonical",
        ));
    }
    Ok(canonical)
}

fn resolve_settings_path(
    context: &AgentContext,
    resource: &ResourceRef,
) -> Result<PathBuf, AgentError> {
    if resource.installation_id != context.installation_id
        || resource.kind != ResourceKind::Settings
    {
        return Err(agent_error(
            AgentErrorCode::InvalidPlan,
            context,
            Some(resource.clone()),
            "Settings resource does not belong to the active Agent context",
        ));
    }
    match (resource.scope, resource.logical_id.as_str()) {
        (ResourceScope::User, "user-config") if resource.project_path.is_none() => {
            Ok(resolve_codex_home(context)?.join("config.toml"))
        }
        (ResourceScope::Project, "project-config")
            if context.project_path.is_some() && resource.project_path == context.project_path =>
        {
            Ok(
                validate_project_path(
                    context,
                    context.project_path.as_deref().unwrap_or_default(),
                )?
                .join(".codex/config.toml"),
            )
        }
        _ => Err(agent_error(
            AgentErrorCode::InvalidPlan,
            context,
            Some(resource.clone()),
            "Unknown Codex settings resource",
        )),
    }
}

fn read_optional(
    path: &PathBuf,
    context: &AgentContext,
    resource: Option<ResourceRef>,
) -> Result<Option<Vec<u8>>, AgentError> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(agent_error(
            AgentErrorCode::Io,
            context,
            resource,
            format!("Failed to read {}: {error}", path.display()),
        )),
    }
}

fn agent_error(
    code: AgentErrorCode,
    context: &AgentContext,
    resource: Option<ResourceRef>,
    message: impl Into<String>,
) -> AgentError {
    AgentError {
        code,
        message: message.into(),
        agent_id: Some(AgentId::from("codex")),
        installation_id: Some(context.installation_id.clone()),
        resource,
        retryable: false,
        details: None,
    }
}
