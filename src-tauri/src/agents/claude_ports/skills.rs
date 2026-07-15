use std::collections::BTreeSet;
use std::io::ErrorKind;
use std::path::PathBuf;

use chrono::{Duration, Utc};

use crate::commands::skills::{is_ad_managed_symlink, scan_skill_library_read_only};
use crate::models::{SkillEntry, SkillScope};

use super::super::{
    AgentContext, AgentError, AgentErrorCode, AgentId, CapabilityAvailability,
    CapabilityLimitation, CapabilityOperation, CollectionInstallRequest, ContentDigest,
    MutationKind, MutationPlan, PlanId, PlannedMutation, ReadPrecondition, ResourceKind,
    ResourceLocation, ResourceOrigin, ResourceRef, ResourceScope, ResourceSnapshot, SkillsPort,
    WritePolicy,
};
use super::common::{agent_error, resolve_claude_home, validate_project_path};

#[derive(Debug, Default)]
pub(crate) struct ClaudeSkillsPort;

impl SkillsPort for ClaudeSkillsPort {
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
        CapabilityAvailability::Degraded
    }

    fn limitations(&self) -> Vec<CapabilityLimitation> {
        vec![CapabilityLimitation {
            code: "git_acquisition_uses_legacy_flow".into(),
            message_key: "agents.capabilities.gitAcquisitionUsesLegacyFlow".into(),
        }]
    }

    fn list(&self, context: &AgentContext) -> Result<Vec<ResourceSnapshot>, AgentError> {
        resolve_claude_home(context)?;
        if let Some(project_path) = &context.project_path {
            validate_project_path(context, project_path)?;
        }
        let entries =
            scan_skill_library_read_only(context.project_path.clone()).map_err(|error| {
                agent_error(
                    AgentErrorCode::Io,
                    context,
                    None,
                    format!("Failed to inspect Claude skills: {error}"),
                )
            })?;
        entries
            .into_iter()
            .map(|entry| skill_snapshot(context, entry))
            .collect()
    }

    fn plan_install(
        &self,
        context: &AgentContext,
        request: CollectionInstallRequest,
    ) -> Result<MutationPlan, AgentError> {
        let source_path = request
            .source
            .get("path")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| {
                agent_error(
                    AgentErrorCode::InvalidPlan,
                    context,
                    None,
                    "Claude local skill install requires source.path",
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
                "Claude skill source must be a directory containing SKILL.md",
            ));
        }
        plan_skill_toggle(context, &request.logical_id, Some(source), true)
    }

    fn plan_set_enabled(
        &self,
        context: &AgentContext,
        resource: &ResourceRef,
        enabled: bool,
    ) -> Result<MutationPlan, AgentError> {
        if resource.installation_id != context.installation_id
            || resource.kind != ResourceKind::Skills
        {
            return Err(agent_error(
                AgentErrorCode::InvalidPlan,
                context,
                Some(resource.clone()),
                "Skill resource does not belong to the active Agent context",
            ));
        }
        let source = if enabled {
            let snapshot = self
                .list(context)?
                .into_iter()
                .find(|snapshot| snapshot.resource.logical_id == resource.logical_id)
                .ok_or_else(|| {
                    agent_error(
                        AgentErrorCode::InvalidPlan,
                        context,
                        Some(resource.clone()),
                        "Claude skill source is not available",
                    )
                })?;
            Some(PathBuf::from(snapshot.location.path))
        } else {
            None
        };
        plan_skill_toggle(context, &resource.logical_id, source, enabled)
    }
}

fn skill_snapshot(
    context: &AgentContext,
    entry: SkillEntry,
) -> Result<ResourceSnapshot, AgentError> {
    let logical_id = entry
        .source_id
        .as_ref()
        .map(|source_id| format!("{source_id}/{}", entry.name))
        .unwrap_or_else(|| entry.name.clone());
    let scope = if entry.scope == SkillScope::Project {
        ResourceScope::Project
    } else {
        ResourceScope::User
    };
    let content = serde_json::to_value(&entry).map_err(|error| {
        agent_error(
            AgentErrorCode::Io,
            context,
            None,
            format!("Failed to serialize Claude skill {logical_id}: {error}"),
        )
    })?;
    let bytes = serde_json::to_vec(&content).map_err(|error| {
        agent_error(
            AgentErrorCode::Io,
            context,
            None,
            format!("Failed to digest Claude skill {logical_id}: {error}"),
        )
    })?;
    Ok(ResourceSnapshot {
        resource: ResourceRef {
            installation_id: context.installation_id.clone(),
            project_path: (scope == ResourceScope::Project)
                .then(|| context.project_path.clone())
                .flatten(),
            kind: ResourceKind::Skills,
            scope,
            logical_id,
        },
        location: ResourceLocation {
            path: entry.path,
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
    })
}

fn plan_skill_toggle(
    context: &AgentContext,
    logical_id: &str,
    source: Option<PathBuf>,
    enabled: bool,
) -> Result<MutationPlan, AgentError> {
    let name = logical_id.rsplit('/').next().unwrap_or_default();
    if name.is_empty() || matches!(name, "." | "..") {
        return Err(agent_error(
            AgentErrorCode::InvalidPlan,
            context,
            None,
            "Invalid Claude skill logical id",
        ));
    }
    let claude_home = resolve_claude_home(context)?;
    let (scope, project_path, target) = if let Some(project_path) = &context.project_path {
        let project = validate_project_path(context, project_path)?;
        (
            ResourceScope::Project,
            Some(project_path.clone()),
            project.join(".claude/skills").join(name),
        )
    } else {
        (
            ResourceScope::User,
            None,
            claude_home.join("skills").join(name),
        )
    };
    let resource = ResourceRef {
        installation_id: context.installation_id.clone(),
        project_path,
        kind: ResourceKind::Skills,
        scope,
        logical_id: logical_id.into(),
    };
    let existing_digest = match std::fs::symlink_metadata(&target) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            if !is_ad_managed_symlink(&target) {
                return Err(agent_error(
                    AgentErrorCode::PermissionDenied,
                    context,
                    Some(resource),
                    format!(
                        "Skill target is not an AD-managed symlink: {}",
                        target.display()
                    ),
                ));
            }
            let current_target = std::fs::read_link(&target).map_err(|error| {
                agent_error(
                    AgentErrorCode::Io,
                    context,
                    Some(resource.clone()),
                    format!("Failed to read skill link {}: {error}", target.display()),
                )
            })?;
            Some(ContentDigest::sha256(
                current_target.to_string_lossy().as_bytes(),
            ))
        }
        Ok(_) => {
            return Err(agent_error(
                AgentErrorCode::PermissionDenied,
                context,
                Some(resource),
                format!(
                    "Skill target is not an AD-managed symlink: {}",
                    target.display()
                ),
            ))
        }
        Err(error) if error.kind() == ErrorKind::NotFound => None,
        Err(error) => {
            return Err(agent_error(
                AgentErrorCode::Io,
                context,
                Some(resource),
                format!(
                    "Failed to inspect skill target {}: {error}",
                    target.display()
                ),
            ))
        }
    };

    let mutation = if enabled {
        let source = source.ok_or_else(|| {
            agent_error(
                AgentErrorCode::InvalidPlan,
                context,
                Some(resource.clone()),
                "Claude skill enable requires a source directory",
            )
        })?;
        Some(PlannedMutation {
            resource: resource.clone(),
            kind: if existing_digest.is_some() {
                MutationKind::Replace
            } else {
                MutationKind::Create
            },
            expected_digest: existing_digest.clone(),
            media_type: "application/vnd.ad.symlink".into(),
            content: Some(serde_json::Value::String(
                source.to_string_lossy().into_owned(),
            )),
        })
    } else {
        existing_digest.as_ref().map(|digest| PlannedMutation {
            resource: resource.clone(),
            kind: MutationKind::Delete,
            expected_digest: Some(digest.clone()),
            media_type: "application/vnd.ad.symlink".into(),
            content: None,
        })
    };
    let read_set = existing_digest
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
        mutations: mutation.into_iter().collect(),
        expires_at: Utc::now() + Duration::minutes(5),
    })
}
