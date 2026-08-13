use std::collections::BTreeSet;
use std::io::ErrorKind;
use std::path::PathBuf;

use chrono::{Duration, Utc};

use crate::models::{SkillEntry, SkillScope};

use super::super::execution_state::ExecutionState;
use super::super::skill_legacy_discovery::{
    is_legacy_ad_managed_symlink, scan_legacy_skill_library_read_only,
};
use super::super::{
    directory_tree_digest, load_ownership_record, validate_ownership_artifact,
    validate_ownership_record, AgentContext, AgentError, AgentErrorCode, AgentId,
    CapabilityAvailability, CapabilityLimitation, CapabilityOperation, CollectionInstallRequest,
    ContentDigest, ManagedResourceTarget, MutationKind, MutationPlan, PlanId, PlannedMutation,
    ReadPrecondition, ResourceKind, ResourceLocation, ResourceOrigin, ResourcePort, ResourceRef,
    ResourceScope, ResourceSnapshot, ResourceStateKind, SkillsPort, WritePolicy,
};
use super::common::{agent_error, resolve_claude_home, validate_project_path};

#[derive(Debug, Default)]
pub(crate) struct ClaudeSkillsPort;

impl ResourcePort for ClaudeSkillsPort {
    fn resolve(
        &self,
        context: &AgentContext,
        resource: &ResourceRef,
    ) -> Result<ManagedResourceTarget, AgentError> {
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
        let name = skill_name(context, &resource.logical_id)?;
        let target = match resource.scope {
            ResourceScope::User if resource.project_path.is_none() => {
                resolve_claude_home(context)?.join("skills").join(name)
            }
            ResourceScope::Project
                if context.project_path.is_some()
                    && resource.project_path == context.project_path =>
            {
                validate_project_path(context, context.project_path.as_deref().unwrap_or_default())?
                    .join(".claude/skills")
                    .join(name)
            }
            _ => {
                return Err(agent_error(
                    AgentErrorCode::InvalidPlan,
                    context,
                    Some(resource.clone()),
                    "Skill scope does not belong to the active Agent context",
                ))
            }
        };
        Ok(ManagedResourceTarget::symlink(target))
    }
}

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
        CapabilityAvailability::Available
    }

    fn limitations(&self) -> Vec<CapabilityLimitation> {
        Vec::new()
    }

    fn list(&self, context: &AgentContext) -> Result<Vec<ResourceSnapshot>, AgentError> {
        resolve_claude_home(context)?;
        if let Some(project_path) = &context.project_path {
            validate_project_path(context, project_path)?;
        }
        let entries =
            scan_legacy_skill_library_read_only(context.project_path.clone()).map_err(|error| {
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
        if context.project_path.is_none() {
            return Err(agent_error(
                AgentErrorCode::InvalidPlan,
                context,
                None,
                "Skill installation requires a project",
            ));
        }
        let (logical_id, source) = install_source(context, &request)?;
        plan_skill_toggle(
            context,
            &logical_id,
            ResourceScope::Project,
            Some(source),
            true,
        )
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
        self.resolve(context, resource)?;
        let source = if enabled {
            let snapshot = self
                .list(context)?
                .into_iter()
                .find(|snapshot| snapshot.resource == *resource)
                .ok_or_else(|| {
                    agent_error(
                        AgentErrorCode::InvalidPlan,
                        context,
                        Some(resource.clone()),
                        "Claude skill source is not available",
                    )
                })?;
            Some(PathBuf::from(
                snapshot
                    .content
                    .get("path")
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| {
                        agent_error(
                            AgentErrorCode::InvalidPlan,
                            context,
                            Some(resource.clone()),
                            "Claude skill snapshot has no library source path",
                        )
                    })?,
            ))
        } else {
            None
        };
        plan_skill_toggle(
            context,
            &resource.logical_id,
            resource.scope,
            source,
            enabled,
        )
    }

    fn plan_update(
        &self,
        context: &AgentContext,
        resource: &ResourceRef,
        request: CollectionInstallRequest,
    ) -> Result<MutationPlan, AgentError> {
        self.resolve(context, resource)?;
        let (logical_id, source) = install_source(context, &request)?;
        if logical_id != resource.logical_id {
            return Err(agent_error(
                AgentErrorCode::InvalidPlan,
                context,
                Some(resource.clone()),
                "Skill update identity differs from the installed resource",
            ));
        }
        plan_skill_toggle(
            context,
            &resource.logical_id,
            resource.scope,
            Some(source),
            true,
        )
    }

    fn plan_remove(
        &self,
        context: &AgentContext,
        resource: &ResourceRef,
    ) -> Result<MutationPlan, AgentError> {
        self.resolve(context, resource)?;
        plan_skill_toggle(context, &resource.logical_id, resource.scope, None, false)
    }
}

fn install_source(
    context: &AgentContext,
    request: &CollectionInstallRequest,
) -> Result<(String, PathBuf), AgentError> {
    let resource_id = request
        .source
        .get("catalogResourceId")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            agent_error(
                AgentErrorCode::InvalidPlan,
                context,
                None,
                "Project Skill installation requires a catalog resource",
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
    if resolved.kind != ResourceKind::Skills {
        return Err(agent_error(
            AgentErrorCode::InvalidPlan,
            context,
            None,
            "Catalog resource is not a Skill",
        ));
    }
    let lexical_source = resolved.stable_path;
    let physical_source = std::fs::canonicalize(&lexical_source).map_err(|error| {
        agent_error(
            AgentErrorCode::InvalidPlan,
            context,
            None,
            format!("Invalid catalog Skill source: {error}"),
        )
    })?;
    if !physical_source.is_dir() || !physical_source.join("SKILL.md").is_file() {
        return Err(agent_error(
            AgentErrorCode::InvalidPlan,
            context,
            None,
            "Claude skill source must be a directory containing SKILL.md",
        ));
    }
    Ok((
        format!("{}/{}", resolved.source_id, resolved.install_id),
        lexical_source,
    ))
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
    let resource = ResourceRef {
        installation_id: context.installation_id.clone(),
        project_path: (scope == ResourceScope::Project)
            .then(|| context.project_path.clone())
            .flatten(),
        kind: ResourceKind::Skills,
        scope,
        logical_id,
    };
    let location = ClaudeSkillsPort.resolve(context, &resource)?;
    Ok(ResourceSnapshot {
        resource,
        location: ResourceLocation {
            path: location.path().to_string_lossy().into_owned(),
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
    scope: ResourceScope,
    source: Option<PathBuf>,
    enabled: bool,
) -> Result<MutationPlan, AgentError> {
    let name = skill_name(context, logical_id)?;
    let claude_home = resolve_claude_home(context)?;
    let (project_path, target) = match scope {
        ResourceScope::User => (None, claude_home.join("skills").join(name)),
        ResourceScope::Project => {
            let project_path = context.project_path.as_ref().ok_or_else(|| {
                agent_error(
                    AgentErrorCode::InvalidPlan,
                    context,
                    None,
                    "Project skill requires a project context",
                )
            })?;
            let project = validate_project_path(context, project_path)?;
            (
                Some(project_path.clone()),
                project.join(".claude/skills").join(name),
            )
        }
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
            let current_target = std::fs::read_link(&target).map_err(|error| {
                agent_error(
                    AgentErrorCode::Io,
                    context,
                    Some(resource.clone()),
                    format!("Failed to read skill link {}: {error}", target.display()),
                )
            })?;
            let digest = ContentDigest::sha256(current_target.to_string_lossy().as_bytes());
            if !replacement_is_authorized(&target, &resource, &digest, enabled) {
                return Err(agent_error(
                    AgentErrorCode::PermissionDenied,
                    context,
                    Some(resource),
                    format!(
                        "Skill target is not proven as AD-managed: {}",
                        target.display()
                    ),
                ));
            }
            Some(digest)
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
        let source_digest = directory_tree_digest(&source).map_err(|error| {
            agent_error(
                AgentErrorCode::InvalidPlan,
                context,
                Some(resource.clone()),
                format!("Failed to digest Claude skill source: {error}"),
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
            content: Some(serde_json::json!({
                "path": source,
                "digest": source_digest,
            })),
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

fn replacement_is_authorized(
    target: &std::path::Path,
    resource: &ResourceRef,
    digest: &ContentDigest,
    validate_source: bool,
) -> bool {
    if is_legacy_ad_managed_symlink(target) {
        return true;
    }
    if resource.scope != ResourceScope::Project {
        return false;
    }
    let Ok(state) = ExecutionState::open() else {
        return false;
    };
    let Ok(Some(record)) = load_ownership_record(&state, resource) else {
        return false;
    };
    validate_ownership_record(
        &record,
        resource,
        target,
        ResourceStateKind::Symlink,
        Some(digest),
    )
    .and_then(|_| {
        if validate_source {
            validate_ownership_artifact(&record)
        } else {
            Ok(())
        }
    })
    .is_ok()
}

fn skill_name<'a>(context: &AgentContext, logical_id: &'a str) -> Result<&'a str, AgentError> {
    let name = logical_id.rsplit('/').next().unwrap_or_default();
    if name.is_empty() || matches!(name, "." | "..") {
        return Err(agent_error(
            AgentErrorCode::InvalidPlan,
            context,
            None,
            "Invalid Claude skill logical id",
        ));
    }
    Ok(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arbitrary_skill_path_is_not_an_install_source() {
        let context = AgentContext {
            installation_id: "claude-code:test".into(),
            project_path: None,
        };
        let request = CollectionInstallRequest {
            logical_id: "root".into(),
            source: serde_json::json!({"path": "/tmp/untrusted"}),
        };

        let error = install_source(&context, &request).unwrap_err();

        assert!(error.message.contains("catalog resource"));
    }
}
