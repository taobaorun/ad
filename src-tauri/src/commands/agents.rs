use crate::agents::{
    builtin_registry, convert_claude_profile_to_codex, profile_settings_content, AgentContext,
    AgentError, AgentErrorCode, AgentId, AgentInstallation, AgentMetadata, CapabilityDescriptor,
    ClaudeToCodexRoute, CollectionInstallRequest, ConversionPreview, ConversionRoute,
    ConversionRoutePreview, ExecutionEngine, InstallationId, MutationPlanView,
    OperationHistoryEntry, OperationReceipt, PlanId, PlanStore, ProcessObservation, ProfileId,
    ReceiptId, ResourceKind, ResourceRef, ResourceScope, ResourceSnapshot, SettingsDocument,
    SettingsEdit,
};
use crate::models::ProfileFile;
use tauri::State;

use super::CmdResult;

#[tauri::command]
pub fn list_agents() -> CmdResult<Vec<AgentMetadata>> {
    Ok(builtin_registry().metadata())
}

#[tauri::command]
pub fn discover_agents() -> CmdResult<Vec<AgentInstallation>> {
    Ok(builtin_registry().discover())
}

#[tauri::command]
pub fn list_agent_capabilities(agent_id: AgentId) -> Result<Vec<CapabilityDescriptor>, AgentError> {
    builtin_registry()
        .capability_descriptors(agent_id.as_str())
        .ok_or_else(|| agent_error_for_id(agent_id, "Unknown Agent adapter"))
}

#[tauri::command]
pub fn inspect_agent_settings(context: AgentContext) -> Result<Vec<ResourceSnapshot>, AgentError> {
    with_context_adapter(&context, |adapter| {
        adapter
            .settings()
            .ok_or_else(|| context_error(&context, "Agent does not support settings inspection"))?
            .inspect(&context)
    })
}

#[tauri::command]
pub fn list_agent_settings_documents(
    context: AgentContext,
) -> Result<Vec<SettingsDocument>, AgentError> {
    with_context_adapter(&context, |adapter| {
        adapter
            .settings()
            .ok_or_else(|| context_error(&context, "Agent does not support settings editing"))?
            .edit_documents(&context)
    })
}

#[tauri::command]
pub fn list_agent_skills(context: AgentContext) -> Result<Vec<ResourceSnapshot>, AgentError> {
    with_context_adapter(&context, |adapter| {
        adapter
            .skills()
            .ok_or_else(|| context_error(&context, "Agent does not support Skill listing"))?
            .list(&context)
    })
}

#[tauri::command]
pub fn list_agent_plugins(context: AgentContext) -> Result<Vec<ResourceSnapshot>, AgentError> {
    with_context_adapter(&context, |adapter| {
        adapter
            .plugins()
            .ok_or_else(|| context_error(&context, "Agent does not support Plugin listing"))?
            .list(&context)
    })
}

#[tauri::command]
pub fn detect_agent_processes(
    context: AgentContext,
) -> Result<Vec<ProcessObservation>, AgentError> {
    with_context_adapter(&context, |adapter| {
        adapter
            .processes()
            .ok_or_else(|| context_error(&context, "Agent does not support process detection"))?
            .detect(&context)
    })
}

#[tauri::command]
pub fn list_agent_operation_history(
    installation_id: Option<InstallationId>,
    limit: Option<usize>,
) -> Result<Vec<OperationHistoryEntry>, AgentError> {
    let directory = crate::fs::paths::history_dir()
        .map_err(|error| operation_history_error(error.to_string()))?
        .join("operations");
    if !directory.is_dir() {
        return Ok(Vec::new());
    }
    let entries = std::fs::read_dir(&directory).map_err(|error| {
        operation_history_error(format!("Failed to read {}: {error}", directory.display()))
    })?;
    let mut history = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("json") {
            continue;
        }
        let receipt = match std::fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<OperationReceipt>(&bytes).ok())
        {
            Some(receipt) => receipt,
            None => {
                tracing::warn!(path = %path.display(), "skipping malformed operation receipt");
                continue;
            }
        };
        if let Some(expected) = &installation_id {
            let matches = receipt
                .applied_resources
                .iter()
                .chain(
                    receipt
                        .post_apply_states
                        .iter()
                        .map(|state| &state.resource),
                )
                .any(|resource| &resource.installation_id == expected);
            if !matches {
                continue;
            }
        }
        let created_at = entry
            .metadata()
            .and_then(|metadata| metadata.modified())
            .map(chrono::DateTime::<chrono::Utc>::from)
            .unwrap_or_else(|_| chrono::Utc::now());
        history.push(OperationHistoryEntry {
            receipt,
            created_at,
        });
    }
    history.sort_by_key(|entry| std::cmp::Reverse(entry.created_at));
    history.truncate(limit.unwrap_or(50).min(200));
    Ok(history)
}

#[tauri::command]
pub fn resolve_agent_context(
    installation_id: InstallationId,
    project_path: Option<String>,
) -> CmdResult<AgentContext> {
    let installation_exists = builtin_registry()
        .discover()
        .iter()
        .any(|installation| installation.id == installation_id);
    if !installation_exists {
        return Err(super::CommandError::Generic(format!(
            "unknown Agent installation: {installation_id}"
        )));
    }

    let project_path = project_path
        .map(|path| {
            let canonical = std::fs::canonicalize(&path).map_err(|error| {
                super::CommandError::Generic(format!("invalid project path {path}: {error}"))
            })?;
            if !canonical.is_dir() {
                return Err(super::CommandError::Generic(format!(
                    "project path is not a directory: {path}"
                )));
            }
            Ok(canonical.to_string_lossy().into_owned())
        })
        .transpose()?;

    Ok(AgentContext {
        installation_id,
        project_path,
    })
}

#[tauri::command]
pub fn preview_claude_to_codex(profile: ProfileFile) -> CmdResult<ConversionPreview> {
    if profile.agent_id != "claude-code" {
        return Err(super::CommandError::Generic(
            "conversion source must be claude-code".into(),
        ));
    }
    Ok(convert_claude_profile_to_codex(&profile))
}

#[tauri::command]
pub fn preview_claude_to_codex_route(
    source_context: AgentContext,
    target_context: AgentContext,
    plans: State<'_, PlanStore>,
) -> Result<ConversionRoutePreview, AgentError> {
    let result = ClaudeToCodexRoute.preview(&source_context, &target_context)?;
    let plan = if result.plan.mutations.is_empty() {
        None
    } else {
        Some(plans.insert_confirmation_required(result.plan)?)
    };
    Ok(ConversionRoutePreview {
        source_agent_id: result.source_agent_id,
        target_agent_id: result.target_agent_id,
        artifacts: result.artifacts,
        plan,
    })
}

#[tauri::command]
pub fn preview_agent_settings_edit(
    context: AgentContext,
    edit: SettingsEdit,
    plans: State<'_, PlanStore>,
) -> Result<MutationPlanView, AgentError> {
    preview_agent_settings_edit_inner(context, edit, plans.inner())
}

#[tauri::command]
pub fn preview_agent_profile_apply(
    context: AgentContext,
    profile_id: ProfileId,
    plans: State<'_, PlanStore>,
) -> Result<MutationPlanView, AgentError> {
    preview_agent_profile_apply_inner(context, profile_id, plans.inner())
}

#[tauri::command]
pub fn preview_agent_collection_install(
    context: AgentContext,
    kind: ResourceKind,
    request: CollectionInstallRequest,
    plans: State<'_, PlanStore>,
) -> Result<MutationPlanView, AgentError> {
    let plan = with_context_adapter(&context, |adapter| match kind {
        ResourceKind::Skills => adapter
            .skills()
            .ok_or_else(|| context_error(&context, "Agent does not support Skill installation"))?
            .plan_install(&context, request),
        ResourceKind::Plugins => adapter
            .plugins()
            .ok_or_else(|| context_error(&context, "Agent does not support Plugin installation"))?
            .plan_install(&context, request),
        _ => Err(context_error(
            &context,
            "Only Skill and Plugin collections support installation",
        )),
    })?;
    plans.insert(plan)
}

#[tauri::command]
pub fn preview_agent_collection_toggle(
    context: AgentContext,
    resource: ResourceRef,
    enabled: bool,
    plans: State<'_, PlanStore>,
) -> Result<MutationPlanView, AgentError> {
    preview_agent_collection_toggle_inner(context, resource, enabled, plans.inner())
}

#[tauri::command]
pub fn apply_agent_plan(
    plan_id: PlanId,
    plans: State<'_, PlanStore>,
) -> Result<OperationReceipt, AgentError> {
    ExecutionEngine.apply(&plan_id, plans.inner())
}

#[tauri::command]
pub fn apply_conversion_plan(
    plan_id: PlanId,
    confirmed: bool,
    plans: State<'_, PlanStore>,
) -> Result<OperationReceipt, AgentError> {
    require_confirmation(confirmed, "Conversion apply requires explicit confirmation")?;
    ExecutionEngine.apply_confirmed(&plan_id, plans.inner())
}

#[tauri::command]
pub fn rollback_agent_receipt(
    receipt_id: ReceiptId,
    confirmed: bool,
) -> Result<OperationReceipt, AgentError> {
    require_confirmation(confirmed, "Rollback requires explicit confirmation")?;
    ExecutionEngine.rollback(&receipt_id)
}

fn preview_agent_settings_edit_inner(
    context: AgentContext,
    edit: SettingsEdit,
    plans: &PlanStore,
) -> Result<MutationPlanView, AgentError> {
    let registry = builtin_registry();
    let installation = registry
        .discover()
        .into_iter()
        .find(|installation| installation.id == context.installation_id)
        .ok_or_else(|| context_error(&context, "Unknown Agent installation"))?;
    let adapter = registry
        .adapter(installation.agent_id.as_str())
        .ok_or_else(|| context_error(&context, "Unknown Agent adapter"))?;
    let settings = adapter
        .settings()
        .ok_or_else(|| context_error(&context, "Agent does not support settings edits"))?;
    let plan = settings.plan_edit(&context, edit)?;
    plans.insert(plan)
}

fn preview_agent_profile_apply_inner(
    context: AgentContext,
    profile_id: ProfileId,
    plans: &PlanStore,
) -> Result<MutationPlanView, AgentError> {
    let plan = with_context_adapter(&context, |adapter| {
        let agent_id = adapter.definition().id.to_string();
        let profile =
            super::profile_envelopes::get_profile_envelope(agent_id, profile_id.to_string())
                .map_err(|error| profile_apply_error(&context, error.to_string()))?;
        let profile_content = profile_settings_content(adapter, &profile)
            .map_err(|error| profile_apply_error(&context, error.to_string()))?;
        let settings = adapter
            .settings()
            .ok_or_else(|| context_error(&context, "Agent does not support settings edits"))?;
        let target_scope = if context.project_path.is_some() {
            ResourceScope::Project
        } else {
            ResourceScope::User
        };
        let target = settings
            .edit_documents(&context)?
            .into_iter()
            .find(|document| document.resource.scope == target_scope)
            .ok_or_else(|| context_error(&context, "Agent profile has no settings target"))?;
        settings.plan_edit(
            &context,
            SettingsEdit {
                resource: target.resource,
                media_type: profile_content.media_type,
                content: profile_content.content,
            },
        )
    })?;
    plans.insert(plan)
}

fn preview_agent_collection_toggle_inner(
    context: AgentContext,
    resource: ResourceRef,
    enabled: bool,
    plans: &PlanStore,
) -> Result<MutationPlanView, AgentError> {
    let plan = with_context_adapter(&context, |adapter| match resource.kind {
        ResourceKind::Skills => adapter
            .skills()
            .ok_or_else(|| context_error(&context, "Agent does not support Skill toggles"))?
            .plan_set_enabled(&context, &resource, enabled),
        ResourceKind::Plugins => adapter
            .plugins()
            .ok_or_else(|| context_error(&context, "Agent does not support Plugin toggles"))?
            .plan_set_enabled(&context, &resource, enabled),
        _ => Err(context_error(
            &context,
            "Only Skill and Plugin resources support enable or disable",
        )),
    })?;
    plans.insert(plan)
}

fn with_context_adapter<T>(
    context: &AgentContext,
    operation: impl FnOnce(&dyn crate::agents::AgentAdapter) -> Result<T, AgentError>,
) -> Result<T, AgentError> {
    let registry = builtin_registry();
    let installation = registry
        .discover()
        .into_iter()
        .find(|installation| installation.id == context.installation_id)
        .ok_or_else(|| context_error(context, "Unknown Agent installation"))?;
    let adapter = registry
        .adapter(installation.agent_id.as_str())
        .ok_or_else(|| context_error(context, "Unknown Agent adapter"))?;
    operation(adapter)
}

fn context_error(context: &AgentContext, message: impl Into<String>) -> AgentError {
    AgentError {
        code: AgentErrorCode::Unsupported,
        message: message.into(),
        agent_id: None,
        installation_id: Some(context.installation_id.clone()),
        resource: None,
        retryable: false,
        details: None,
    }
}

fn agent_error_for_id(agent_id: AgentId, message: impl Into<String>) -> AgentError {
    AgentError {
        code: AgentErrorCode::Unsupported,
        message: message.into(),
        agent_id: Some(agent_id),
        installation_id: None,
        resource: None,
        retryable: false,
        details: None,
    }
}

fn operation_history_error(message: impl Into<String>) -> AgentError {
    AgentError {
        code: AgentErrorCode::Io,
        message: message.into(),
        agent_id: None,
        installation_id: None,
        resource: None,
        retryable: true,
        details: None,
    }
}

fn profile_apply_error(context: &AgentContext, message: impl Into<String>) -> AgentError {
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

fn require_confirmation(confirmed: bool, message: &str) -> Result<(), AgentError> {
    if confirmed {
        return Ok(());
    }
    Err(AgentError {
        code: AgentErrorCode::PermissionDenied,
        message: message.into(),
        agent_id: None,
        installation_id: None,
        resource: None,
        retryable: false,
        details: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::{CapabilityKind, CapabilityOperation};

    #[test]
    fn lists_built_in_agents() {
        let agents = list_agents().unwrap();

        assert_eq!(
            agents
                .iter()
                .map(|agent| agent.id.as_str())
                .collect::<Vec<_>>(),
            ["claude-code", "codex",]
        );
    }

    #[test]
    #[serial_test::serial(home_env)]
    fn resolves_a_discovered_installation_context() {
        let temp = tempfile::tempdir().unwrap();
        let claude_home = temp.path().join(".claude");
        let project = temp.path().join("project");
        std::fs::create_dir_all(&claude_home).unwrap();
        std::fs::create_dir_all(&project).unwrap();
        let previous_home = std::env::var("AD_HOME").ok();
        let previous_codex_home = std::env::var("CODEX_HOME").ok();
        std::env::set_var("AD_HOME", temp.path());
        std::env::remove_var("CODEX_HOME");

        let installation = discover_agents().unwrap().remove(0);
        let context = resolve_agent_context(
            installation.id,
            Some(project.to_string_lossy().into_owned()),
        )
        .unwrap();

        let expected_project = std::fs::canonicalize(project)
            .unwrap()
            .to_string_lossy()
            .into_owned();
        match previous_home {
            Some(value) => std::env::set_var("AD_HOME", value),
            None => std::env::remove_var("AD_HOME"),
        }
        match previous_codex_home {
            Some(value) => std::env::set_var("CODEX_HOME", value),
            None => std::env::remove_var("CODEX_HOME"),
        }
        assert_eq!(
            context.project_path.as_deref(),
            Some(expected_project.as_str())
        );
    }

    #[test]
    #[serial_test::serial(home_env)]
    fn settings_preview_returns_a_stored_plan_view_without_writing() {
        let temp = tempfile::tempdir().unwrap();
        let claude_home = temp.path().join(".claude");
        std::fs::create_dir_all(&claude_home).unwrap();
        let original = br#"{"model":"claude-opus-4-7"}"#;
        std::fs::write(claude_home.join("settings.json"), original).unwrap();
        let previous_home = std::env::var("AD_HOME").ok();
        let previous_codex_home = std::env::var("CODEX_HOME").ok();
        std::env::set_var("AD_HOME", temp.path());
        std::env::remove_var("CODEX_HOME");

        let registry = builtin_registry();
        let installation = registry
            .discover()
            .into_iter()
            .find(|item| item.agent_id.as_str() == "claude-code")
            .unwrap();
        let context = AgentContext {
            installation_id: installation.id,
            project_path: None,
        };
        let resource = registry
            .adapter("claude-code")
            .unwrap()
            .settings()
            .unwrap()
            .inspect(&context)
            .unwrap()
            .remove(0)
            .resource;
        let store = PlanStore::default();

        let view = preview_agent_settings_edit_inner(
            context,
            SettingsEdit {
                resource,
                media_type: "application/json".into(),
                content: serde_json::json!({"model": "claude-sonnet-4-5"}),
            },
            &store,
        )
        .unwrap();

        assert_eq!(view.agent_id.as_str(), "claude-code");
        assert_eq!(view.changes.len(), 1);
        assert_eq!(
            std::fs::read(claude_home.join("settings.json")).unwrap(),
            original
        );

        match previous_home {
            Some(value) => std::env::set_var("AD_HOME", value),
            None => std::env::remove_var("AD_HOME"),
        }
        match previous_codex_home {
            Some(value) => std::env::set_var("CODEX_HOME", value),
            None => std::env::remove_var("CODEX_HOME"),
        }
    }

    #[test]
    #[serial_test::serial(home_env)]
    fn profile_apply_preview_targets_the_active_agent_settings_without_writing() {
        let temp = tempfile::tempdir().unwrap();
        let codex_home = temp.path().join(".codex");
        std::fs::create_dir_all(&codex_home).unwrap();
        let original = b"model = \"gpt-5.3\"\n";
        std::fs::write(codex_home.join("config.toml"), original).unwrap();
        let previous_home = std::env::var("AD_HOME").ok();
        let previous_codex_home = std::env::var("CODEX_HOME").ok();
        std::env::set_var("AD_HOME", temp.path());
        std::env::set_var("CODEX_HOME", &codex_home);

        let now = chrono::Utc::now();
        crate::commands::profile_envelopes::save_profile_envelope(crate::agents::AgentProfile {
            schema_version: crate::agents::AGENT_PROFILE_SCHEMA_VERSION,
            key: crate::agents::AgentProfileKey {
                agent_id: AgentId::from("codex"),
                profile_id: crate::agents::ProfileId::from("review"),
            },
            metadata: crate::agents::ProfileMetadata {
                display_name: "Review".into(),
                description: None,
                color: "#7C3AED".into(),
                created_at: now,
                updated_at: now,
            },
            payload_schema: crate::agents::CODEX_PROFILE_PAYLOAD_SCHEMA.into(),
            payload: serde_json::json!({"configToml": "model = \"gpt-5.4\"\n"}),
        })
        .unwrap();
        let installation = builtin_registry()
            .discover()
            .into_iter()
            .find(|item| item.agent_id.as_str() == "codex")
            .unwrap();
        let context = AgentContext {
            installation_id: installation.id,
            project_path: None,
        };

        let view = preview_agent_profile_apply_inner(
            context,
            crate::agents::ProfileId::from("review"),
            &PlanStore::default(),
        )
        .unwrap();

        assert_eq!(view.agent_id.as_str(), "codex");
        assert_eq!(view.changes[0].resource.logical_id, "user-config");
        assert_eq!(
            std::fs::read(codex_home.join("config.toml")).unwrap(),
            original
        );

        match previous_home {
            Some(value) => std::env::set_var("AD_HOME", value),
            None => std::env::remove_var("AD_HOME"),
        }
        match previous_codex_home {
            Some(value) => std::env::set_var("CODEX_HOME", value),
            None => std::env::remove_var("CODEX_HOME"),
        }
    }

    #[test]
    fn capability_descriptors_are_exposed_by_agent_id() {
        let descriptors = list_agent_capabilities(AgentId::from("codex")).unwrap();

        assert_eq!(descriptors.len(), 5);
        let settings = descriptors
            .iter()
            .find(|descriptor| descriptor.kind == CapabilityKind::Settings)
            .unwrap();
        assert!(settings.operations.contains(&CapabilityOperation::Inspect));
        assert!(settings.operations.contains(&CapabilityOperation::Apply));
    }

    #[test]
    #[serial_test::serial(home_env)]
    fn agent_resources_are_inspected_and_toggled_without_direct_writes() {
        let temp = tempfile::tempdir().unwrap();
        let codex_home = temp.path().join(".codex");
        let skill_dir = temp.path().join(".agents/skills/review");
        std::fs::create_dir_all(&codex_home).unwrap();
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(skill_dir.join("SKILL.md"), "# Review\n").unwrap();
        let original = b"model = \"gpt-5.4\"\n";
        std::fs::write(codex_home.join("config.toml"), original).unwrap();
        let previous_home = std::env::var("AD_HOME").ok();
        let previous_codex_home = std::env::var("CODEX_HOME").ok();
        std::env::set_var("AD_HOME", temp.path());
        std::env::set_var("CODEX_HOME", &codex_home);

        let installation = builtin_registry()
            .discover()
            .into_iter()
            .find(|item| item.agent_id.as_str() == "codex")
            .unwrap();
        let context = AgentContext {
            installation_id: installation.id,
            project_path: None,
        };
        let settings = inspect_agent_settings(context.clone()).unwrap();
        let skills = list_agent_skills(context.clone()).unwrap();
        let store = PlanStore::default();
        let plan = preview_agent_collection_toggle_inner(
            context,
            skills[0].resource.clone(),
            false,
            &store,
        )
        .unwrap();

        assert_eq!(settings.len(), 1);
        assert_eq!(settings[0].media_type, "application/toml");
        assert_eq!(skills.len(), 1);
        assert_eq!(plan.changes.len(), 1);
        assert_eq!(
            std::fs::read(codex_home.join("config.toml")).unwrap(),
            original
        );

        match previous_home {
            Some(value) => std::env::set_var("AD_HOME", value),
            None => std::env::remove_var("AD_HOME"),
        }
        match previous_codex_home {
            Some(value) => std::env::set_var("CODEX_HOME", value),
            None => std::env::remove_var("CODEX_HOME"),
        }
    }

    #[test]
    #[serial_test::serial(home_env)]
    fn settings_documents_include_missing_project_targets() {
        let temp = tempfile::tempdir().unwrap();
        let codex_home = temp.path().join(".codex");
        let project = temp.path().join("project");
        std::fs::create_dir_all(&codex_home).unwrap();
        std::fs::create_dir_all(&project).unwrap();
        let previous_home = std::env::var("AD_HOME").ok();
        let previous_codex_home = std::env::var("CODEX_HOME").ok();
        std::env::set_var("AD_HOME", temp.path());
        std::env::set_var("CODEX_HOME", &codex_home);

        let installation = builtin_registry()
            .discover()
            .into_iter()
            .find(|item| item.agent_id.as_str() == "codex")
            .unwrap();
        let context = AgentContext {
            installation_id: installation.id,
            project_path: Some(
                std::fs::canonicalize(&project)
                    .unwrap()
                    .to_string_lossy()
                    .into(),
            ),
        };
        let documents = list_agent_settings_documents(context).unwrap();

        assert_eq!(documents.len(), 2);
        let project_document = documents
            .iter()
            .find(|document| document.resource.scope == crate::agents::ResourceScope::Project)
            .unwrap();
        assert!(!project_document.exists);
        assert_eq!(
            project_document.content,
            serde_json::Value::String(String::new())
        );

        match previous_home {
            Some(value) => std::env::set_var("AD_HOME", value),
            None => std::env::remove_var("AD_HOME"),
        }
        match previous_codex_home {
            Some(value) => std::env::set_var("CODEX_HOME", value),
            None => std::env::remove_var("CODEX_HOME"),
        }
    }

    #[test]
    #[serial_test::serial(home_env)]
    fn operation_history_filters_receipts_by_installation() {
        let temp = tempfile::tempdir().unwrap();
        let previous_home = std::env::var("AD_HOME").ok();
        std::env::set_var("AD_HOME", temp.path());
        let operations = crate::fs::paths::history_dir().unwrap().join("operations");
        std::fs::create_dir_all(&operations).unwrap();
        std::fs::write(
            operations.join("receipt-1.json"),
            serde_json::to_vec(&serde_json::json!({
                "id": "receipt-1",
                "planId": "plan-1",
                "status": "complete",
                "appliedResources": [{
                    "installationId": "codex:default",
                    "kind": "settings",
                    "scope": "user",
                    "logicalId": "user-config"
                }],
                "backupPaths": [],
                "postApplyStates": []
            }))
            .unwrap(),
        )
        .unwrap();

        let entries =
            list_agent_operation_history(Some(InstallationId::from("codex:default")), Some(20))
                .unwrap();
        let other = list_agent_operation_history(
            Some(InstallationId::from("claude-code:default")),
            Some(20),
        )
        .unwrap();

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].receipt.id.as_str(), "receipt-1");
        assert!(other.is_empty());

        match previous_home {
            Some(value) => std::env::set_var("AD_HOME", value),
            None => std::env::remove_var("AD_HOME"),
        }
    }
}
