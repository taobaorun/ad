use std::collections::BTreeSet;
use std::io::ErrorKind;
use std::path::PathBuf;

use chrono::{Duration, Utc};

use super::codex::discover_codex_candidates;
use super::project_codex_runtime::{
    project_runtime_descriptor_for_context, runtime_for_installation,
};
use super::{
    AgentContext, AgentError, AgentErrorCode, AgentId, CapabilityAvailability, CapabilityOperation,
    ContentDigest, ManagedResourceTarget, MutationKind, MutationPlan, PlanId, PlannedMutation,
    ReadPrecondition, ResourceKind, ResourceLocation, ResourceOrigin, ResourcePort, ResourceRef,
    ResourceScope, ResourceSnapshot, SettingsDocument, SettingsEdit, SettingsPort, WritePolicy,
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
        if let Some(runtime) = project_runtime_for_context(context)? {
            push_snapshot(
                &mut snapshots,
                context,
                ResourceScope::Project,
                "runtime-config",
                runtime.runtime_home.join("config.toml"),
                ResourceOrigin::Project,
            )?;
            return Ok(snapshots);
        }
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

    fn edit_documents(&self, context: &AgentContext) -> Result<Vec<SettingsDocument>, AgentError> {
        let mut documents = self
            .inspect(context)?
            .into_iter()
            .filter(|snapshot| snapshot.resource.logical_id != "project-config")
            .map(SettingsDocument::from)
            .collect::<Vec<_>>();
        if project_runtime_for_context(context)?.is_some() {
            return Ok(documents);
        }
        let codex_home = resolve_codex_home(context)?;
        push_missing_document(
            &mut documents,
            context,
            ResourceScope::User,
            "user-config",
            codex_home.join("config.toml"),
            ResourceOrigin::User,
        );
        Ok(documents)
    }

    fn plan_edit(
        &self,
        context: &AgentContext,
        edit: SettingsEdit,
    ) -> Result<MutationPlan, AgentError> {
        if edit.media_type == "application/vnd.ad.project-settings+json" {
            return super::codex_plugins::plan_project_runtime_semantic_settings_edit(
                context, &edit,
            )?
            .ok_or_else(|| {
                agent_error(
                    AgentErrorCode::Unsupported,
                    context,
                    Some(edit.resource),
                    "Project Codex settings require a prepared Project Runtime",
                )
            });
        }
        if edit.resource.scope == ResourceScope::Project
            && edit.resource.logical_id == "project-config"
        {
            return Err(agent_error(
                AgentErrorCode::Unsupported,
                context,
                Some(edit.resource),
                "Native Project Codex config is inspect-only",
            ));
        }
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
        if let Some(plan) =
            super::codex_plugins::plan_project_runtime_settings_edit(context, &edit)?
        {
            return Ok(plan);
        }
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

fn push_missing_document(
    documents: &mut Vec<SettingsDocument>,
    context: &AgentContext,
    scope: ResourceScope,
    logical_id: &str,
    path: PathBuf,
    origin: ResourceOrigin,
) {
    if documents.iter().any(|document| {
        document.resource.scope == scope && document.resource.logical_id == logical_id
    }) {
        return;
    }
    documents.push(SettingsDocument {
        resource: ResourceRef {
            installation_id: context.installation_id.clone(),
            project_path: (scope == ResourceScope::Project)
                .then(|| context.project_path.clone())
                .flatten(),
            kind: ResourceKind::Settings,
            scope,
            logical_id: logical_id.into(),
        },
        location: ResourceLocation {
            path: path.to_string_lossy().into_owned(),
            origin,
        },
        media_type: "application/toml".into(),
        content: serde_json::Value::String(String::new()),
        exists: false,
        digest: None,
    });
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

pub(super) fn resolve_codex_home(context: &AgentContext) -> Result<PathBuf, AgentError> {
    if let Some(runtime) = project_runtime_for_context(context)? {
        return Ok(runtime.runtime_home);
    }
    let home = discover_codex_candidates()
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
        })?;
    if let Some(runtime) = runtime_for_installation(&context.installation_id) {
        if context.project_path.as_deref() != Some(runtime.project_path.as_str()) {
            return Err(agent_error(
                AgentErrorCode::InvalidPlan,
                context,
                None,
                "Project Codex runtime belongs to a different project",
            ));
        }
    }
    Ok(home)
}

pub(super) fn resolve_codex_user_home(context: &AgentContext) -> Result<PathBuf, AgentError> {
    if let Some(runtime) = project_runtime_for_context(context)? {
        return discover_codex_candidates()
            .into_iter()
            .find(|candidate| candidate.installation().id == runtime.base_installation_id)
            .map(|candidate| PathBuf::from(&candidate.installation().root_path))
            .ok_or_else(|| {
                agent_error(
                    AgentErrorCode::InvalidPlan,
                    context,
                    None,
                    "Project Codex runtime base installation is unavailable",
                )
            });
    }
    resolve_codex_home(context)
}

pub(super) fn project_runtime_for_context(
    context: &AgentContext,
) -> Result<Option<super::ProjectCodexRuntimeDescriptor>, AgentError> {
    let Some(project_path) = context.project_path.as_deref() else {
        return Ok(None);
    };
    project_runtime_descriptor_for_context(
        &context.installation_id,
        std::path::Path::new(project_path),
    )
    .map_err(|error| {
        agent_error(
            AgentErrorCode::InvalidPlan,
            context,
            None,
            error.to_string(),
        )
    })
}

pub(super) fn validate_project_path(
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
        (ResourceScope::Project, "runtime-config")
            if context.project_path.is_some() && resource.project_path == context.project_path =>
        {
            Ok(resolve_codex_home(context)?.join("config.toml"))
        }
        _ => Err(agent_error(
            AgentErrorCode::InvalidPlan,
            context,
            Some(resource.clone()),
            "Unknown Codex settings resource",
        )),
    }
}

pub(super) fn read_optional(
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

pub(super) fn agent_error(
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
