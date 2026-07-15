use std::collections::{BTreeMap, BTreeSet};
use std::io::ErrorKind;
use std::path::PathBuf;

use chrono::{Duration, Utc};

use super::{
    AgentContext, AgentError, AgentErrorCode, AgentId, CapabilityAvailability,
    CapabilityLimitation, CapabilityOperation, CollectionInstallRequest, ContentDigest,
    DiscoveryEvidence, InstallationCandidate, LaunchPort, LaunchRecipe, MutationKind, MutationPlan,
    PlanId, PlannedMutation, PluginsPort, ProcessObservation, ProcessPort, ReadPrecondition,
    ResourceKind, ResourceLocation, ResourceOrigin, ResourceRef, ResourceScope, ResourceSnapshot,
    SettingsEdit, SettingsPort, SkillsPort, WritePolicy,
};
use crate::commands::activate::detect_claude_processes_inner;
use crate::commands::skills::{is_ad_managed_symlink, list_plugins, scan_skill_library_read_only};
use crate::fs::paths::claude_dir;
use crate::models::{SkillEntry, SkillScope};

#[derive(Debug, Default)]
pub(crate) struct ClaudeSettingsPort;

#[derive(Debug, Default)]
pub(crate) struct ClaudeSkillsPort;

#[derive(Debug, Default)]
pub(crate) struct ClaudePluginsPort;

#[derive(Debug, Default)]
pub(crate) struct ClaudeProcessPort;

#[derive(Debug, Default)]
pub(crate) struct ClaudeLaunchPort;

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
                content: Some(edit.content),
            }],
            expires_at: Utc::now() + Duration::minutes(5),
        };
        plan.validate()?;
        Ok(plan)
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
        let (scope, project_path, location, origin) =
            if let Some(project_path) = &context.project_path {
                let project = validate_project_path(context, project_path)?;
                (
                    ResourceScope::Project,
                    Some(project_path.clone()),
                    project.join(".claude/settings.local.json"),
                    ResourceOrigin::Project,
                )
            } else {
                (
                    ResourceScope::User,
                    None,
                    claude_home.join("settings.json"),
                    ResourceOrigin::User,
                )
            };
        let plugins = list_plugins(context.project_path.clone()).map_err(|error| {
            agent_error(
                AgentErrorCode::Io,
                context,
                None,
                format!("Failed to inspect Claude plugins: {error}"),
            )
        })?;
        plugins
            .into_iter()
            .map(|plugin| {
                let content = serde_json::to_value(&plugin).map_err(|error| {
                    agent_error(
                        AgentErrorCode::Io,
                        context,
                        None,
                        format!("Failed to serialize Claude plugin {}: {error}", plugin.id),
                    )
                })?;
                let bytes = serde_json::to_vec(&content).map_err(|error| {
                    agent_error(
                        AgentErrorCode::Io,
                        context,
                        None,
                        format!("Failed to digest Claude plugin {}: {error}", plugin.id),
                    )
                })?;
                Ok(ResourceSnapshot {
                    resource: ResourceRef {
                        installation_id: context.installation_id.clone(),
                        project_path: project_path.clone(),
                        kind: ResourceKind::Plugins,
                        scope,
                        logical_id: plugin.id,
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

impl ProcessPort for ClaudeProcessPort {
    fn scopes(&self) -> BTreeSet<ResourceScope> {
        BTreeSet::from([ResourceScope::User, ResourceScope::Project])
    }

    fn operations(&self) -> BTreeSet<CapabilityOperation> {
        BTreeSet::from([CapabilityOperation::Detect])
    }

    fn availability(&self) -> CapabilityAvailability {
        CapabilityAvailability::Available
    }

    fn detect(&self, context: &AgentContext) -> Result<Vec<ProcessObservation>, AgentError> {
        resolve_claude_home(context)?;
        Ok(detect_claude_processes_inner()
            .into_iter()
            .map(|process| ProcessObservation {
                pid: process.pid,
                installation_id: context.installation_id.clone(),
                executable: process.cmd,
                cwd: None,
            })
            .collect())
    }
}

impl LaunchPort for ClaudeLaunchPort {
    fn scopes(&self) -> BTreeSet<ResourceScope> {
        BTreeSet::from([ResourceScope::Project])
    }

    fn operations(&self) -> BTreeSet<CapabilityOperation> {
        BTreeSet::from([CapabilityOperation::Launch])
    }

    fn availability(&self) -> CapabilityAvailability {
        CapabilityAvailability::Available
    }

    fn recipe(&self, context: &AgentContext) -> Result<LaunchRecipe, AgentError> {
        resolve_claude_home(context)?;
        let project_path = context.project_path.as_deref().ok_or_else(|| {
            agent_error(
                AgentErrorCode::InvalidPlan,
                context,
                None,
                "Claude launch requires a project context",
            )
        })?;
        let project = validate_project_path(context, project_path)?;
        Ok(LaunchRecipe {
            program: "claude".into(),
            args: Vec::new(),
            env: BTreeMap::new(),
            cwd: project.to_string_lossy().into_owned(),
        })
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

fn resolve_claude_home(context: &AgentContext) -> Result<PathBuf, AgentError> {
    let path = claude_dir().map_err(|error| {
        agent_error(
            AgentErrorCode::Io,
            context,
            None,
            format!("Failed to resolve Claude config home: {error}"),
        )
    })?;
    let candidate = InstallationCandidate::from_existing_home(
        "claude-code",
        path,
        DiscoveryEvidence::DefaultHome,
    )
    .ok_or_else(|| {
        agent_error(
            AgentErrorCode::Unsupported,
            context,
            None,
            "Claude config home is not available",
        )
    })?;
    if candidate.installation().id != context.installation_id {
        return Err(agent_error(
            AgentErrorCode::InvalidPlan,
            context,
            None,
            "Agent context does not target the discovered Claude installation",
        ));
    }
    Ok(PathBuf::from(&candidate.installation().root_path))
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
        project_path: context.project_path.clone(),
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

fn resolve_settings_path(
    context: &AgentContext,
    resource: &ResourceRef,
) -> Result<PathBuf, AgentError> {
    if resource.installation_id != context.installation_id
        || resource.kind != ResourceKind::Settings
        || resource.project_path != context.project_path
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

fn agent_error(
    code: AgentErrorCode,
    context: &AgentContext,
    resource: Option<ResourceRef>,
    message: impl Into<String>,
) -> AgentError {
    AgentError {
        code,
        message: message.into(),
        agent_id: Some(AgentId::from("claude-code")),
        installation_id: Some(context.installation_id.clone()),
        resource,
        retryable: false,
        details: None,
    }
}

#[cfg(test)]
mod tests {
    use super::super::*;

    fn setup() -> (tempfile::TempDir, AgentContext, Vec<u8>) {
        let temp = tempfile::tempdir().unwrap();
        let claude_home = temp.path().join(".claude");
        std::fs::create_dir_all(&claude_home).unwrap();
        let content = br#"{"model":"claude-opus-4-7"}"#.to_vec();
        std::fs::write(claude_home.join("settings.json"), &content).unwrap();
        std::env::set_var("AD_HOME", temp.path());
        std::env::remove_var("CODEX_HOME");

        let installation = builtin_registry()
            .discover()
            .into_iter()
            .find(|installation| installation.agent_id.as_str() == "claude-code")
            .unwrap();
        let context = AgentContext {
            installation_id: installation.id,
            project_path: None,
        };
        (temp, context, content)
    }

    #[test]
    #[serial_test::serial(home_env)]
    fn settings_port_inspects_claude_json_with_sha256_digest() {
        let (_temp, context, _) = setup();
        let registry = builtin_registry();
        let port = registry.adapter("claude-code").unwrap().settings().unwrap();

        let first = port.inspect(&context).unwrap();
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].resource.kind, ResourceKind::Settings);
        assert_eq!(first[0].resource.scope, ResourceScope::User);
        assert_eq!(first[0].media_type, "application/json");
        assert!(first[0].digest.as_str().starts_with("sha256:"));

        std::fs::write(
            _temp.path().join(".claude/settings.json"),
            br#"{"model":"claude-sonnet-4-5"}"#,
        )
        .unwrap();
        let second = port.inspect(&context).unwrap();
        assert_ne!(first[0].digest, second[0].digest);
    }

    #[test]
    #[serial_test::serial(home_env)]
    fn settings_port_plans_an_edit_without_writing_source() {
        let (temp, context, original) = setup();
        let registry = builtin_registry();
        let port = registry.adapter("claude-code").unwrap().settings().unwrap();
        let snapshot = port.inspect(&context).unwrap().remove(0);

        let plan = port
            .plan_edit(
                &context,
                SettingsEdit {
                    resource: snapshot.resource,
                    media_type: "application/json".into(),
                    content: serde_json::json!({"model": "claude-sonnet-4-5"}),
                },
            )
            .unwrap();

        assert_eq!(plan.mutations.len(), 1);
        assert_eq!(plan.mutations[0].kind, MutationKind::Replace);
        assert!(plan.mutations[0].expected_digest.is_some());
        assert_eq!(
            std::fs::read(temp.path().join(".claude/settings.json")).unwrap(),
            original
        );
    }

    #[test]
    #[serial_test::serial(home_env)]
    fn settings_port_rejects_non_object_json() {
        let (_temp, context, _) = setup();
        let registry = builtin_registry();
        let port = registry.adapter("claude-code").unwrap().settings().unwrap();
        let snapshot = port.inspect(&context).unwrap().remove(0);

        let error = port
            .plan_edit(
                &context,
                SettingsEdit {
                    resource: snapshot.resource,
                    media_type: "application/json".into(),
                    content: serde_json::json!("not-an-object"),
                },
            )
            .unwrap_err();

        assert_eq!(error.code, AgentErrorCode::InvalidPlan);
    }

    #[test]
    #[serial_test::serial(home_env)]
    fn process_port_returns_standard_observations() {
        let (_temp, context, _) = setup();
        let registry = builtin_registry();
        let port = registry
            .adapter("claude-code")
            .unwrap()
            .processes()
            .unwrap();

        let observations = port.detect(&context).unwrap();

        assert!(observations
            .iter()
            .all(|process| process.installation_id == context.installation_id));
        assert!(observations
            .iter()
            .all(|process| !process.executable.is_empty()));
    }

    #[test]
    #[serial_test::serial(home_env)]
    fn launch_port_builds_a_claude_recipe_for_the_project_context() {
        let (temp, mut context, _) = setup();
        let project = temp.path().join("project");
        std::fs::create_dir_all(&project).unwrap();
        context.project_path = Some(
            std::fs::canonicalize(&project)
                .unwrap()
                .to_string_lossy()
                .into_owned(),
        );
        let registry = builtin_registry();
        let port = registry.adapter("claude-code").unwrap().launcher().unwrap();

        let recipe = port.recipe(&context).unwrap();

        assert_eq!(recipe.program, "claude");
        assert_eq!(recipe.cwd, context.project_path.unwrap());
        assert!(recipe.args.is_empty());
        assert!(recipe.env.is_empty());
    }

    #[test]
    #[serial_test::serial(home_env)]
    fn skills_port_lists_and_plans_project_enable_without_writing() {
        let (temp, mut context, _) = setup();
        let skill = temp.path().join(".ad/skill-library/source/demo");
        let project = temp.path().join("project");
        std::fs::create_dir_all(&skill).unwrap();
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(
            skill.join("SKILL.md"),
            "---\nname: demo\ndescription: Demo skill\n---\n",
        )
        .unwrap();
        context.project_path = Some(
            std::fs::canonicalize(&project)
                .unwrap()
                .to_string_lossy()
                .into_owned(),
        );
        let registry = builtin_registry();
        let port = registry.adapter("claude-code").unwrap().skills().unwrap();

        let snapshots = port.list(&context).unwrap();
        assert!(!temp.path().join(".ad/state/skill_sources.json").exists());
        let demo = snapshots
            .into_iter()
            .find(|snapshot| snapshot.resource.logical_id == "source/demo")
            .unwrap();
        let plan = port
            .plan_set_enabled(&context, &demo.resource, true)
            .unwrap();

        assert_eq!(plan.mutations.len(), 1);
        assert_eq!(plan.mutations[0].kind, MutationKind::Create);
        assert_eq!(plan.mutations[0].media_type, "application/vnd.ad.symlink");
        assert!(!project.join(".claude/skills/demo").exists());
    }

    #[test]
    #[serial_test::serial(home_env)]
    fn skills_port_refuses_to_replace_an_unmanaged_symlink() {
        let (temp, mut context, _) = setup();
        let skill = temp.path().join(".ad/skill-library/source/demo");
        let project = temp.path().join("project");
        let unmanaged = temp.path().join("unmanaged");
        std::fs::create_dir_all(&skill).unwrap();
        std::fs::create_dir_all(project.join(".claude/skills")).unwrap();
        std::fs::create_dir_all(&unmanaged).unwrap();
        std::fs::write(skill.join("SKILL.md"), "---\nname: demo\n---\n").unwrap();
        std::os::unix::fs::symlink(&unmanaged, project.join(".claude/skills/demo")).unwrap();
        context.project_path = Some(
            std::fs::canonicalize(&project)
                .unwrap()
                .to_string_lossy()
                .into_owned(),
        );
        let registry = builtin_registry();
        let port = registry.adapter("claude-code").unwrap().skills().unwrap();
        let demo = port
            .list(&context)
            .unwrap()
            .into_iter()
            .find(|snapshot| snapshot.resource.logical_id == "source/demo")
            .unwrap();

        let error = port
            .plan_set_enabled(&context, &demo.resource, true)
            .unwrap_err();

        assert_eq!(error.code, AgentErrorCode::PermissionDenied);
    }

    #[test]
    #[serial_test::serial(home_env)]
    fn plugins_port_lists_and_plans_project_override_without_writing() {
        let (temp, mut context, _) = setup();
        let project = temp.path().join("project");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(
            temp.path().join(".claude/settings.json"),
            br#"{"enabledPlugins":{"demo":true}}"#,
        )
        .unwrap();
        context.project_path = Some(
            std::fs::canonicalize(&project)
                .unwrap()
                .to_string_lossy()
                .into_owned(),
        );
        let registry = builtin_registry();
        let port = registry.adapter("claude-code").unwrap().plugins().unwrap();

        let plugin = port
            .list(&context)
            .unwrap()
            .into_iter()
            .find(|snapshot| snapshot.resource.logical_id == "demo")
            .unwrap();
        let plan = port
            .plan_set_enabled(&context, &plugin.resource, false)
            .unwrap();

        assert_eq!(plan.mutations.len(), 1);
        assert_eq!(plan.mutations[0].kind, MutationKind::Create);
        assert_eq!(
            plan.mutations[0].content.as_ref().unwrap()["enabledPlugins"]["demo"],
            false
        );
        assert!(!project.join(".claude/settings.local.json").exists());
    }
}
