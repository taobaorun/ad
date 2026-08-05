use std::collections::BTreeSet;
use std::io::ErrorKind;
use std::path::{Component, Path, PathBuf};

use chrono::{Duration, Utc};

use crate::fs::paths::home;

use super::codex_ports::{agent_error, resolve_codex_home, validate_project_path};
use super::codex_skill_config::{disabled_skill_paths, plan_skill_config};
use super::execution_fs::directory_tree_digest;
use super::{
    AgentContext, AgentError, AgentErrorCode, AgentId, CapabilityAvailability, CapabilityOperation,
    CollectionInstallRequest, ContentDigest, ManagedResourceTarget, MutationKind, MutationPlan,
    PlanId, PlannedMutation, ReadPrecondition, ResourceKind, ResourceLocation, ResourceOrigin,
    ResourcePort, ResourceRef, ResourceScope, ResourceSnapshot, SkillsPort, WritePolicy,
};

#[derive(Debug, Default)]
pub(crate) struct CodexSkillsPort;

impl ResourcePort for CodexSkillsPort {
    fn resolve(
        &self,
        context: &AgentContext,
        resource: &ResourceRef,
    ) -> Result<ManagedResourceTarget, AgentError> {
        validate_resource(context, resource)?;
        Ok(ManagedResourceTarget::symlink(
            skill_root(context, resource.scope)?.join(skill_name(context, &resource.logical_id)?),
        ))
    }
}

impl SkillsPort for CodexSkillsPort {
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

    fn list(&self, context: &AgentContext) -> Result<Vec<ResourceSnapshot>, AgentError> {
        resolve_codex_home(context)?;
        let disabled = disabled_skill_paths(context)?;
        let mut snapshots = scan_scope(context, ResourceScope::User, &disabled)?;
        if context.project_path.is_some() {
            snapshots.extend(scan_scope(context, ResourceScope::Project, &disabled)?);
        }
        Ok(snapshots)
    }

    fn plan_install(
        &self,
        context: &AgentContext,
        request: CollectionInstallRequest,
    ) -> Result<MutationPlan, AgentError> {
        skill_name(context, &request.logical_id)?;
        let source = install_source(context, &request)?;
        let scope = if context.project_path.is_some() {
            ResourceScope::Project
        } else {
            ResourceScope::User
        };
        let resource = ResourceRef {
            installation_id: context.installation_id.clone(),
            project_path: (scope == ResourceScope::Project)
                .then(|| context.project_path.clone())
                .flatten(),
            kind: ResourceKind::Skills,
            scope,
            logical_id: request.logical_id,
        };
        plan_skill_link(context, resource, source, false)
    }

    fn plan_set_enabled(
        &self,
        context: &AgentContext,
        resource: &ResourceRef,
        enabled: bool,
    ) -> Result<MutationPlan, AgentError> {
        validate_resource(context, resource)?;
        let skill_md = self.resolve(context, resource)?.path().join("SKILL.md");
        let skill_md = std::fs::canonicalize(&skill_md).map_err(|error| {
            agent_error(
                AgentErrorCode::InvalidPlan,
                context,
                Some(resource.clone()),
                format!("Invalid Codex skill path {}: {error}", skill_md.display()),
            )
        })?;
        plan_skill_config(context, &skill_md, enabled)
    }

    fn plan_update(
        &self,
        context: &AgentContext,
        resource: &ResourceRef,
        request: CollectionInstallRequest,
    ) -> Result<MutationPlan, AgentError> {
        validate_resource(context, resource)?;
        if request.logical_id != resource.logical_id {
            return Err(agent_error(
                AgentErrorCode::InvalidPlan,
                context,
                Some(resource.clone()),
                "Skill update identity differs from the installed resource",
            ));
        }
        plan_skill_link(
            context,
            resource.clone(),
            install_source(context, &request)?,
            true,
        )
    }

    fn plan_remove(
        &self,
        context: &AgentContext,
        resource: &ResourceRef,
    ) -> Result<MutationPlan, AgentError> {
        validate_resource(context, resource)?;
        let target = self.resolve(context, resource)?.path().to_path_buf();
        let metadata = std::fs::symlink_metadata(&target).map_err(|error| {
            agent_error(
                AgentErrorCode::ResourceChanged,
                context,
                Some(resource.clone()),
                format!("Installed Skill is unavailable: {error}"),
            )
        })?;
        if !metadata.file_type().is_symlink() {
            return Err(agent_error(
                AgentErrorCode::PermissionDenied,
                context,
                Some(resource.clone()),
                "Installed Skill target is not a symlink",
            ));
        }
        let link = std::fs::read_link(&target).map_err(|error| {
            agent_error(
                AgentErrorCode::Io,
                context,
                Some(resource.clone()),
                error.to_string(),
            )
        })?;
        let expected_digest = ContentDigest::sha256(link.to_string_lossy().as_bytes());
        let skill_md = std::fs::canonicalize(target.join("SKILL.md")).map_err(|error| {
            agent_error(
                AgentErrorCode::ResourceChanged,
                context,
                Some(resource.clone()),
                error.to_string(),
            )
        })?;
        let mut plan = plan_skill_config(context, &skill_md, true)?;
        plan.read_set.push(ReadPrecondition {
            resource: resource.clone(),
            expected_digest: expected_digest.clone(),
            write_policy: WritePolicy::Mutable,
        });
        plan.mutations.insert(
            0,
            PlannedMutation {
                resource: resource.clone(),
                kind: MutationKind::Delete,
                expected_digest: Some(expected_digest),
                media_type: "application/vnd.ad.symlink".into(),
                content: None,
            },
        );
        Ok(plan)
    }
}

fn install_source(
    context: &AgentContext,
    request: &CollectionInstallRequest,
) -> Result<PathBuf, AgentError> {
    let source_path = request
        .source
        .get("path")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            agent_error(
                AgentErrorCode::InvalidPlan,
                context,
                None,
                "Codex local skill install requires source.path",
            )
        })?;
    let requested_source = PathBuf::from(source_path);
    let lexical_source = direct_skill_source(&requested_source).map_err(|error| {
        agent_error(
            AgentErrorCode::InvalidPlan,
            context,
            None,
            format!("Invalid skill source path {source_path}: {error}"),
        )
    })?;
    let physical_source = std::fs::canonicalize(&lexical_source).map_err(|error| {
        agent_error(
            AgentErrorCode::InvalidPlan,
            context,
            None,
            format!("Invalid skill source path {source_path}: {error}"),
        )
    })?;
    if !physical_source.is_dir() || !physical_source.join("SKILL.md").is_file() {
        return Err(agent_error(
            AgentErrorCode::InvalidPlan,
            context,
            None,
            "Codex skill source must be a directory containing SKILL.md",
        ));
    }
    Ok(lexical_source)
}

fn direct_skill_source(source: &Path) -> std::io::Result<PathBuf> {
    let metadata = std::fs::symlink_metadata(source)?;
    if !metadata.file_type().is_symlink() {
        return Ok(source.to_path_buf());
    }
    let target = std::fs::read_link(source)?;
    if target.is_absolute() {
        Ok(target)
    } else {
        Ok(source
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(target))
    }
}

fn plan_skill_link(
    context: &AgentContext,
    resource: ResourceRef,
    source: PathBuf,
    allow_replace: bool,
) -> Result<MutationPlan, AgentError> {
    let target = CodexSkillsPort
        .resolve(context, &resource)?
        .path()
        .to_path_buf();
    let existing_digest = match std::fs::symlink_metadata(&target) {
        Ok(metadata) if metadata.file_type().is_symlink() => Some(ContentDigest::sha256(
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
        )),
        Ok(_) => {
            return Err(agent_error(
                AgentErrorCode::PermissionDenied,
                context,
                Some(resource),
                "Codex skill target is not a symlink",
            ))
        }
        Err(error) if error.kind() == ErrorKind::NotFound => None,
        Err(error) => {
            return Err(agent_error(
                AgentErrorCode::Io,
                context,
                Some(resource),
                error.to_string(),
            ))
        }
    };
    if existing_digest.is_some() && !allow_replace {
        return Err(agent_error(
            AgentErrorCode::PermissionDenied,
            context,
            Some(resource),
            "Codex skill target already exists",
        ));
    }
    let source_digest = directory_tree_digest(&source).map_err(|error| {
        agent_error(
            AgentErrorCode::InvalidPlan,
            context,
            None,
            format!("Failed to digest Codex skill source: {error}"),
        )
    })?;
    Ok(MutationPlan {
        id: PlanId::from(uuid::Uuid::new_v4().to_string()),
        agent_id: AgentId::from("codex"),
        context: context.clone(),
        read_set: existing_digest
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
            resource,
            kind: if existing_digest.is_some() {
                MutationKind::Replace
            } else {
                MutationKind::Create
            },
            expected_digest: existing_digest,
            media_type: "application/vnd.ad.symlink".into(),
            content: Some(serde_json::json!({
                "path": source,
                "digest": source_digest,
            })),
        }],
        expires_at: Utc::now() + Duration::minutes(5),
    })
}

fn scan_scope(
    context: &AgentContext,
    scope: ResourceScope,
    disabled: &BTreeSet<String>,
) -> Result<Vec<ResourceSnapshot>, AgentError> {
    let root = skill_root(context, scope)?;
    let entries = match std::fs::read_dir(&root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(agent_error(
                AgentErrorCode::Io,
                context,
                None,
                format!("Failed to scan Codex skills at {}: {error}", root.display()),
            ))
        }
    };
    let mut snapshots = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let skill_md = path.join("SKILL.md");
        if !skill_md.is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        let canonical_md = std::fs::canonicalize(&skill_md).map_err(|error| {
            agent_error(
                AgentErrorCode::Io,
                context,
                None,
                format!("Failed to resolve {}: {error}", skill_md.display()),
            )
        })?;
        let enabled = !disabled.contains(&canonical_md.to_string_lossy().into_owned());
        let content = serde_json::json!({"name": name, "enabled": enabled});
        let bytes = serde_json::to_vec(&content)
            .map_err(|error| agent_error(AgentErrorCode::Io, context, None, error.to_string()))?;
        snapshots.push(ResourceSnapshot {
            resource: ResourceRef {
                installation_id: context.installation_id.clone(),
                project_path: (scope == ResourceScope::Project)
                    .then(|| context.project_path.clone())
                    .flatten(),
                kind: ResourceKind::Skills,
                scope,
                logical_id: name,
            },
            location: ResourceLocation {
                path: path.to_string_lossy().into_owned(),
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
        });
    }
    snapshots.sort_by(|left, right| left.resource.logical_id.cmp(&right.resource.logical_id));
    Ok(snapshots)
}

fn skill_root(context: &AgentContext, scope: ResourceScope) -> Result<PathBuf, AgentError> {
    match scope {
        ResourceScope::User => home()
            .map(|home| home.join(".agents/skills"))
            .map_err(|error| agent_error(AgentErrorCode::Io, context, None, error.to_string())),
        ResourceScope::Project => {
            validate_project_path(context, context.project_path.as_deref().unwrap_or_default())
                .map(|project| project.join(".agents/skills"))
        }
    }
}

fn validate_resource(context: &AgentContext, resource: &ResourceRef) -> Result<(), AgentError> {
    skill_name(context, &resource.logical_id)?;
    let scope_matches = match resource.scope {
        ResourceScope::User => resource.project_path.is_none(),
        ResourceScope::Project => {
            context.project_path.is_some() && resource.project_path == context.project_path
        }
    };
    if resource.installation_id != context.installation_id
        || resource.kind != ResourceKind::Skills
        || !scope_matches
    {
        return Err(agent_error(
            AgentErrorCode::InvalidPlan,
            context,
            Some(resource.clone()),
            "Skill resource does not belong to the active Agent context",
        ));
    }
    Ok(())
}

fn skill_name<'a>(context: &AgentContext, logical_id: &'a str) -> Result<&'a str, AgentError> {
    let name = logical_id.rsplit('/').next().unwrap_or_default();
    let mut components = Path::new(name).components();
    if name.is_empty()
        || !matches!(components.next(), Some(Component::Normal(_)))
        || components.next().is_some()
    {
        return Err(agent_error(
            AgentErrorCode::InvalidPlan,
            context,
            None,
            "Invalid Codex skill logical id",
        ));
    }
    Ok(name)
}
