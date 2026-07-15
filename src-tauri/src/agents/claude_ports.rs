use std::collections::BTreeSet;
use std::io::ErrorKind;
use std::path::PathBuf;

use chrono::{Duration, Utc};

use super::{
    AgentContext, AgentError, AgentErrorCode, AgentId, CapabilityAvailability, CapabilityOperation,
    ContentDigest, DiscoveryEvidence, InstallationCandidate, MutationKind, MutationPlan, PlanId,
    PlannedMutation, ReadPrecondition, ResourceKind, ResourceLocation, ResourceOrigin, ResourceRef,
    ResourceScope, ResourceSnapshot, SettingsEdit, SettingsPort, WritePolicy,
};
use crate::fs::paths::claude_dir;

#[derive(Debug, Default)]
pub(crate) struct ClaudeSettingsPort;

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
}
