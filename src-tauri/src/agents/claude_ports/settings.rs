use std::collections::BTreeSet;
use std::path::PathBuf;

use chrono::{Duration, Utc};

use super::super::{
    AgentContext, AgentError, AgentErrorCode, AgentId, CapabilityAvailability, CapabilityOperation,
    ContentDigest, ManagedResourceTarget, MutationKind, MutationPlan, PlanId, PlannedMutation,
    ReadPrecondition, ResourceKind, ResourceLocation, ResourceOrigin, ResourcePort, ResourceRef,
    ResourceScope, ResourceSnapshot, SettingsDocument, SettingsEdit, SettingsPort, WritePolicy,
};
use super::common::{agent_error, read_optional, resolve_claude_home, validate_project_path};

#[derive(Debug, Default)]
pub(crate) struct ClaudeSettingsPort;

impl ResourcePort for ClaudeSettingsPort {
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

impl SettingsPort for ClaudeSettingsPort {
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
        let claude_home = resolve_claude_home(context)?;
        let mut snapshots = Vec::new();
        push_snapshot(
            &mut snapshots,
            context,
            ResourceScope::User,
            "user-settings",
            claude_home.join("settings.json"),
            ResourceOrigin::User,
        )?;

        if let Some(project_path) = &context.project_path {
            let project = validate_project_path(context, project_path)?;
            push_snapshot(
                &mut snapshots,
                context,
                ResourceScope::Project,
                "project-shared",
                project.join(".claude/settings.json"),
                ResourceOrigin::Project,
            )?;
            push_snapshot(
                &mut snapshots,
                context,
                ResourceScope::Project,
                "project-local",
                project.join(".claude/settings.local.json"),
                ResourceOrigin::Project,
            )?;
        }

        Ok(snapshots)
    }

    fn edit_documents(&self, context: &AgentContext) -> Result<Vec<SettingsDocument>, AgentError> {
        let mut documents = self
            .inspect(context)?
            .into_iter()
            .map(SettingsDocument::from)
            .collect::<Vec<_>>();
        let claude_home = resolve_claude_home(context)?;
        push_missing_document(
            &mut documents,
            context,
            ResourceScope::User,
            "user-settings",
            claude_home.join("settings.json"),
            ResourceOrigin::User,
        );
        if let Some(project_path) = &context.project_path {
            let project = validate_project_path(context, project_path)?;
            push_missing_document(
                &mut documents,
                context,
                ResourceScope::Project,
                "project-shared",
                project.join(".claude/settings.json"),
                ResourceOrigin::Project,
            );
            push_missing_document(
                &mut documents,
                context,
                ResourceScope::Project,
                "project-local",
                project.join(".claude/settings.local.json"),
                ResourceOrigin::Project,
            );
        }
        Ok(documents)
    }

    fn plan_edit(
        &self,
        context: &AgentContext,
        edit: SettingsEdit,
    ) -> Result<MutationPlan, AgentError> {
        if edit.media_type != "application/json" {
            return Err(agent_error(
                AgentErrorCode::InvalidPlan,
                context,
                Some(edit.resource),
                "Claude settings edits require application/json",
            ));
        }
        if !edit.content.is_object() {
            return Err(agent_error(
                AgentErrorCode::InvalidPlan,
                context,
                Some(edit.resource),
                "Claude settings content must be a JSON object",
            ));
        }
        let path = resolve_settings_path(context, &edit.resource)?;
        let existing = read_optional(&path, context, Some(edit.resource.clone()))?;
        let mut content = edit.content;
        if let Some(existing) = existing.as_deref() {
            let current = serde_json::from_slice(existing).map_err(|error| {
                agent_error(
                    AgentErrorCode::InvalidPlan,
                    context,
                    Some(edit.resource.clone()),
                    format!("Invalid existing Claude settings JSON: {error}"),
                )
            })?;
            super::super::settings_inventory::restore_masked_settings_values(
                &mut content,
                &current,
            );
        }
        let expected_digest = existing.as_deref().map(ContentDigest::sha256);
        let mutation_kind = if existing.is_some() {
            MutationKind::Replace
        } else {
            MutationKind::Create
        };
        let read_set = expected_digest
            .clone()
            .map(|digest| {
                vec![ReadPrecondition {
                    resource: edit.resource.clone(),
                    expected_digest: digest,
                    write_policy: WritePolicy::Mutable,
                }]
            })
            .unwrap_or_default();

        let plan = MutationPlan {
            id: PlanId::from(uuid::Uuid::new_v4().to_string()),
            agent_id: AgentId::from("claude-code"),
            context: context.clone(),
            read_set,
            mutations: vec![PlannedMutation {
                resource: edit.resource,
                kind: mutation_kind,
                expected_digest,
                media_type: edit.media_type,
                content: Some(content),
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
        media_type: "application/json".into(),
        content: serde_json::json!({}),
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
    let content = serde_json::from_slice(&bytes).map_err(|error| {
        agent_error(
            AgentErrorCode::InvalidPlan,
            context,
            None,
            format!(
                "Invalid Claude settings JSON at {}: {error}",
                path.display()
            ),
        )
    })?;
    snapshots.push(ResourceSnapshot {
        resource,
        location: ResourceLocation {
            path: path.to_string_lossy().into_owned(),
            origin,
        },
        media_type: "application/json".into(),
        content,
        digest: ContentDigest::sha256(&bytes),
        observed_at: Utc::now(),
    });
    Ok(())
}

fn resolve_settings_path(
    context: &AgentContext,
    resource: &ResourceRef,
) -> Result<PathBuf, AgentError> {
    let belongs_to_context = match resource.scope {
        ResourceScope::User => resource.project_path.is_none(),
        ResourceScope::Project => {
            context.project_path.is_some() && resource.project_path == context.project_path
        }
    };
    if resource.installation_id != context.installation_id
        || resource.kind != ResourceKind::Settings
        || !belongs_to_context
    {
        return Err(agent_error(
            AgentErrorCode::InvalidPlan,
            context,
            Some(resource.clone()),
            "Settings resource does not belong to the active Agent context",
        ));
    }

    match (resource.scope, resource.logical_id.as_str()) {
        (ResourceScope::User, "user-settings") => {
            Ok(resolve_claude_home(context)?.join("settings.json"))
        }
        (ResourceScope::Project, "project-shared") => Ok(validate_project_path(
            context,
            context.project_path.as_deref().unwrap_or_default(),
        )?
        .join(".claude/settings.json")),
        (ResourceScope::Project, "project-local") => Ok(validate_project_path(
            context,
            context.project_path.as_deref().unwrap_or_default(),
        )?
        .join(".claude/settings.local.json")),
        _ => Err(agent_error(
            AgentErrorCode::InvalidPlan,
            context,
            Some(resource.clone()),
            "Unknown Claude settings resource",
        )),
    }
}
