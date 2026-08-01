use crate::agents::{
    builtin_registry, convert_claude_profile_to_codex, profile_settings_content,
    AcknowledgementRequirement, AgentContext, AgentError, AgentErrorCode, AgentId,
    AgentInstallation, AgentMetadata, CapabilityDescriptor, ClaudeToCodexOptions,
    ClaudeToCodexRoute, CollectionInstallRequest, ContentDigest, ConversionPreview,
    ConversionProgressEvent, ConversionRoutePreview, ExecutionEngine, InstallationId,
    MutationPlanView, OperationHistoryEntry, OperationReceipt, PlanAcknowledgement,
    PlanAcknowledgementCode, PlanId, PlanRiskLevel, PlanStore, ProcessObservation, ProfileId,
    ProjectCodexRuntime, ProjectCodexRuntimeStatus, ReadPrecondition, ReceiptId, ResourceKind,
    ResourceRef, ResourceScope, ResourceSnapshot, SettingsDocument, SettingsEdit, WritePolicy,
};
use crate::models::ProfileFile;
use tauri::{ipc::Channel, Manager, State};

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
pub fn inspect_project_codex_runtime(
    context: AgentContext,
) -> Result<Option<ProjectCodexRuntimeStatus>, AgentError> {
    let installation = builtin_registry()
        .discover()
        .into_iter()
        .find(|installation| installation.id == context.installation_id)
        .ok_or_else(|| context_error(&context, "Unknown Agent installation"))?;
    if installation.agent_id.as_str() != "codex" {
        return Ok(None);
    }
    let Some(project_path) = context.project_path.as_deref() else {
        return Ok(None);
    };
    let runtime = match crate::agents::runtime_for_installation(&context.installation_id) {
        Some(_) => crate::agents::project_runtime_descriptor_for_context(
            &context.installation_id,
            std::path::Path::new(project_path),
        )
        .map_err(|error| context_error(&context, error.to_string()))?
        .ok_or_else(|| {
            context_error(
                &context,
                "Project Codex runtime belongs to a different project",
            )
        })
        .map(Some)?,
        None => crate::agents::project_runtime_descriptor_for_base_project(
            &context.installation_id,
            std::path::Path::new(project_path),
        )
        .map_err(|error| context_error(&context, error.to_string()))?,
    };
    let desired_inherit_base_config = super::projects::load()
        .map_err(|error| context_error(&context, error.to_string()))?
        .into_iter()
        .find(|project| project.path == project_path)
        .map(|project| project.inherit_base_config)
        .unwrap_or(true);
    runtime
        .as_ref()
        .map(|runtime| {
            crate::agents::inspect_project_codex_runtime_status(
                runtime,
                desired_inherit_base_config,
            )
        })
        .transpose()
        .map_err(|error| context_error(&context, error.to_string()))
}

#[tauri::command]
pub fn list_agent_operation_history(
    installation_id: Option<InstallationId>,
    project_path: Option<String>,
    limit: Option<usize>,
) -> Result<Vec<OperationHistoryEntry>, AgentError> {
    crate::agents::list_operation_history(installation_id.as_ref(), project_path.as_deref(), limit)
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
pub async fn preview_claude_to_codex_route(
    source_context: AgentContext,
    target_context: AgentContext,
    options: Option<ClaudeToCodexOptions>,
    progress: Channel<ConversionProgressEvent>,
    plans: State<'_, PlanStore>,
) -> Result<ConversionRoutePreview, AgentError> {
    let options = options.unwrap_or_default();
    let result =
        preview_conversion_route_off_thread(source_context, target_context, options, progress)
            .await?;
    let plan = if result.plan.mutations.is_empty() {
        None
    } else {
        let mut requirements = vec![AcknowledgementRequirement {
            code: PlanAcknowledgementCode::ConversionApply,
            risk: PlanRiskLevel::Confirmation,
        }];
        if result.summary.dangerous > 0 {
            requirements.push(AcknowledgementRequirement {
                code: PlanAcknowledgementCode::DangerousPermissionExpansion,
                risk: PlanRiskLevel::Dangerous,
            });
        }
        Some(plans.insert_with_acknowledgements(result.plan, requirements)?)
    };
    Ok(ConversionRoutePreview {
        source_agent_id: result.source_agent_id,
        target_agent_id: result.target_agent_id,
        artifacts: result.artifacts,
        summary: result.summary,
        plan,
    })
}

async fn preview_conversion_route_off_thread(
    source_context: AgentContext,
    target_context: AgentContext,
    options: ClaudeToCodexOptions,
    progress: Channel<ConversionProgressEvent>,
) -> Result<crate::agents::ConversionRoutePlan, AgentError> {
    let installation_id = target_context.installation_id.clone();
    tokio::task::spawn_blocking(move || {
        let target_context =
            ensure_project_codex_target_context(target_context, options.profile_id.as_deref())?;
        let report = |event| send_conversion_progress(&progress, event);
        let mut result = ClaudeToCodexRoute
            .preview_with_options_and_progress(&source_context, &target_context, &options, &report)
            .map_err(mark_conversion_preview_error)?;
        append_project_policy_precondition(&target_context, &options, &mut result.plan)?;
        Ok(result)
    })
    .await
    .map_err(|error| {
        mark_conversion_preview_error(AgentError {
            code: AgentErrorCode::Io,
            message: format!("Conversion preview worker failed: {error}"),
            agent_id: None,
            installation_id: Some(installation_id),
            resource: None,
            retryable: false,
            details: None,
        })
    })?
}

fn append_project_policy_precondition(
    context: &AgentContext,
    options: &ClaudeToCodexOptions,
    plan: &mut crate::agents::MutationPlan,
) -> Result<(), AgentError> {
    let Some(project_path) = context.project_path.as_deref() else {
        return Ok(());
    };
    let (projects, bytes) = super::projects::load_snapshot()
        .map_err(|error| context_error(context, error.to_string()))?;
    let Some(project) = projects
        .into_iter()
        .find(|project| project.path == project_path)
    else {
        return Ok(());
    };
    if project.inherit_base_config != options.inherit_base_config {
        return Err(context_error(
            context,
            "Project Codex inheritance setting changed before Preview",
        ));
    }
    plan.read_set.push(ReadPrecondition {
        resource: ResourceRef {
            installation_id: context.installation_id.clone(),
            project_path: context.project_path.clone(),
            kind: ResourceKind::Plugins,
            scope: ResourceScope::Project,
            logical_id: "project-policy".into(),
        },
        expected_digest: ContentDigest::sha256(&bytes),
        write_policy: WritePolicy::ReadOnly,
    });
    Ok(())
}

fn send_conversion_progress(
    progress: &Channel<ConversionProgressEvent>,
    event: ConversionProgressEvent,
) {
    // Progress is advisory; a closed UI consumer must not change the preview result.
    let _ = progress.send(event);
}

fn mark_conversion_preview_error(mut error: AgentError) -> AgentError {
    match error.details.as_mut() {
        Some(serde_json::Value::Object(details)) => {
            details.insert(
                "phase".into(),
                serde_json::Value::String("conversion_preview".into()),
            );
        }
        _ => {
            error.details = Some(serde_json::json!({"phase": "conversion_preview"}));
        }
    }
    error
}

fn ensure_project_codex_target_context(
    context: AgentContext,
    profile_id: Option<&str>,
) -> Result<AgentContext, AgentError> {
    if profile_id.is_some_and(|profile_id| {
        profile_id.is_empty()
            || profile_id.len() > 100
            || !profile_id.chars().all(|character| {
                character.is_ascii_alphanumeric() || matches!(character, '-' | '_')
            })
    }) {
        return Err(context_error(&context, "Invalid Codex Profile id"));
    }
    let Some(project_path) = context.project_path.as_deref() else {
        return Ok(context);
    };
    if crate::agents::runtime_for_installation(&context.installation_id).is_some() {
        return Ok(context);
    }
    let base = builtin_registry()
        .discover()
        .into_iter()
        .find(|installation| installation.id == context.installation_id)
        .ok_or_else(|| context_error(&context, "Unknown Codex base installation"))?;
    if base.agent_id.as_str() != "codex" {
        return Err(context_error(
            &context,
            "Project conversion target must be a Codex installation",
        ));
    }
    let runtime = ProjectCodexRuntime::derive(&base, std::path::Path::new(project_path))
        .map_err(|error| context_error(&context, error.to_string()))?;
    Ok(AgentContext {
        installation_id: runtime.runtime_installation_id,
        project_path: Some(runtime.project_path),
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
pub async fn apply_conversion_plan(
    app: tauri::AppHandle,
    plan_id: PlanId,
    acknowledgements: Vec<PlanAcknowledgement>,
) -> Result<OperationReceipt, AgentError> {
    run_agent_operation_off_thread(move || {
        let plans = app.state::<PlanStore>();
        ExecutionEngine.apply_acknowledged(&plan_id, plans.inner(), &acknowledgements)
    })
    .await
}

#[tauri::command]
pub fn rollback_agent_receipt(
    receipt_id: ReceiptId,
    confirmed: bool,
) -> Result<OperationReceipt, AgentError> {
    require_confirmation(confirmed, "Rollback requires explicit confirmation")?;
    ExecutionEngine.rollback(&receipt_id)
}

async fn run_agent_operation_off_thread<T, F>(operation: F) -> Result<T, AgentError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, AgentError> + Send + 'static,
{
    tokio::task::spawn_blocking(operation)
        .await
        .map_err(|error| AgentError {
            code: AgentErrorCode::Io,
            message: format!("Agent operation worker failed: {error}"),
            agent_id: None,
            installation_id: None,
            resource: None,
            retryable: true,
            details: None,
        })?
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
        let profile_id = profile_id.to_string();
        let profile = super::profile_envelopes::get_profile_envelope(agent_id, profile_id.clone())
            .map_err(|error| profile_apply_error(&context, error.to_string()))?;
        let profile_content = profile_settings_content(adapter, &profile)
            .map_err(|error| profile_apply_error(&context, error.to_string()))?;
        if adapter.definition().id.as_str() == "codex" {
            let config_toml = profile_content.content.as_str().ok_or_else(|| {
                profile_apply_error(&context, "Codex Profile settings must contain TOML text")
            })?;
            if let Some(plan) = crate::agents::plan_project_runtime_profile_apply(
                &context,
                &profile_id,
                config_toml,
            )? {
                return Ok(plan);
            }
        }
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
    let adapter = registry.adapter_for_context(context)?;
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

#[cfg(test)]
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

    #[test]
    fn conversion_preview_errors_keep_the_diagnostic_phase() {
        let error = AgentError {
            code: crate::agents::AgentErrorCode::InvalidPlan,
            message: "Enabled base Plugin marketplace source is unavailable".into(),
            agent_id: None,
            installation_id: None,
            resource: None,
            retryable: false,
            details: Some(serde_json::json!({"pluginId": "browser@openai-bundled"})),
        };

        let marked = mark_conversion_preview_error(error);

        assert_eq!(
            marked.details.as_ref().unwrap()["phase"],
            "conversion_preview"
        );
        assert_eq!(
            marked.details.as_ref().unwrap()["pluginId"],
            "browser@openai-bundled"
        );
    }

    #[test]
    fn conversion_progress_channel_failure_is_non_fatal() {
        let channel = tauri::ipc::Channel::new(|_| Err(tauri::Error::FailedToReceiveMessage));

        send_conversion_progress(
            &channel,
            crate::agents::ConversionProgressEvent {
                phase: crate::agents::ConversionProgressPhase::ReadingConfiguration,
                current: 0,
                total: None,
                item: None,
            },
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn conversion_apply_work_does_not_block_the_async_runtime() {
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let apply = tokio::spawn(run_agent_operation_off_thread(move || {
            started_tx.send(()).unwrap();
            release_rx
                .recv_timeout(std::time::Duration::from_secs(2))
                .map_err(|error| operation_history_error(error.to_string()))?;
            Ok(())
        }));

        tokio::time::timeout(std::time::Duration::from_secs(1), started_rx)
            .await
            .expect("apply work should start without blocking the async runtime")
            .unwrap();
        release_tx.send(()).unwrap();
        apply.await.unwrap().unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    #[serial_test::serial(home_env)]
    async fn conversion_preview_progress_does_not_block_the_async_runtime() {
        let temp = tempfile::tempdir().unwrap();
        let claude_home = temp.path().join(".claude");
        let codex_home = temp.path().join(".codex");
        std::fs::create_dir_all(&claude_home).unwrap();
        std::fs::create_dir_all(&codex_home).unwrap();
        std::fs::write(claude_home.join("settings.json"), "{}").unwrap();
        std::fs::write(codex_home.join("config.toml"), "model = \"gpt-5.6\"\n").unwrap();
        let previous_home = std::env::var("AD_HOME").ok();
        let previous_codex_home = std::env::var("CODEX_HOME").ok();
        std::env::set_var("AD_HOME", temp.path());
        std::env::set_var("CODEX_HOME", &codex_home);
        let installations = builtin_registry().discover();
        let source_context = AgentContext {
            installation_id: installations
                .iter()
                .find(|installation| installation.agent_id.as_str() == "claude-code")
                .unwrap()
                .id
                .clone(),
            project_path: None,
        };
        let target_context = AgentContext {
            installation_id: installations
                .iter()
                .find(|installation| installation.agent_id.as_str() == "codex")
                .unwrap()
                .id
                .clone(),
            project_path: None,
        };
        let (reported_tx, reported_rx) = tokio::sync::oneshot::channel();
        let reported_tx = std::sync::Mutex::new(Some(reported_tx));
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let release_rx = std::sync::Mutex::new(release_rx);
        let progress = tauri::ipc::Channel::new(move |_| {
            if let Some(reported_tx) = reported_tx.lock().unwrap().take() {
                let _ = reported_tx.send(());
                release_rx
                    .lock()
                    .unwrap()
                    .recv_timeout(std::time::Duration::from_secs(2))
                    .map_err(|_| tauri::Error::FailedToReceiveMessage)?;
            }
            Ok(())
        });

        let preview = tokio::spawn(preview_conversion_route_off_thread(
            source_context,
            target_context,
            ClaudeToCodexOptions::default(),
            progress,
        ));
        tokio::time::timeout(std::time::Duration::from_secs(1), reported_rx)
            .await
            .expect("progress should arrive while preview is running")
            .unwrap();
        release_tx.send(()).unwrap();
        preview.await.unwrap().unwrap();

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
    fn project_conversion_automatically_derives_a_codex_runtime_context() {
        let temp = tempfile::tempdir().unwrap();
        let codex_home = temp.path().join(".codex");
        let project = temp.path().join("project");
        std::fs::create_dir_all(&codex_home).unwrap();
        std::fs::create_dir_all(&project).unwrap();
        let previous_home = std::env::var("AD_HOME").ok();
        let previous_codex_home = std::env::var("CODEX_HOME").ok();
        std::env::set_var("AD_HOME", temp.path());
        std::env::remove_var("CODEX_HOME");
        let base = builtin_registry()
            .discover()
            .into_iter()
            .find(|installation| installation.agent_id.as_str() == "codex")
            .unwrap();
        let context = AgentContext {
            installation_id: base.id.clone(),
            project_path: Some(
                std::fs::canonicalize(&project)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned(),
            ),
        };

        let derived = ensure_project_codex_target_context(context, Some("project-api")).unwrap();

        assert_ne!(derived.installation_id, base.id);
        let runtime = ProjectCodexRuntime::derive(&base, &project).unwrap();
        assert_eq!(derived.installation_id, runtime.runtime_installation_id);
        assert_eq!(runtime.base_installation_id, base.id);
        assert!(!runtime.runtime_home.exists());
        assert!(crate::agents::runtime_for_installation(&derived.installation_id).is_none());
        assert!(!temp
            .path()
            .join(".ad/state/codex-project-runtimes")
            .exists());

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
    fn project_runtime_inspection_rejects_a_different_project() {
        let temp = tempfile::tempdir().unwrap();
        let codex_home = temp.path().join(".codex");
        let project_a = temp.path().join("project-a");
        let project_b = temp.path().join("project-b");
        std::fs::create_dir_all(&codex_home).unwrap();
        std::fs::create_dir_all(&project_a).unwrap();
        std::fs::create_dir_all(&project_b).unwrap();
        let previous_home = std::env::var("AD_HOME").ok();
        let previous_codex_home = std::env::var("CODEX_HOME").ok();
        std::env::set_var("AD_HOME", temp.path());
        std::env::remove_var("CODEX_HOME");
        let base = builtin_registry()
            .discover()
            .into_iter()
            .find(|installation| installation.agent_id.as_str() == "codex")
            .unwrap();
        let runtime = ProjectCodexRuntime::derive(&base, &project_a).unwrap();
        std::fs::create_dir_all(&runtime.runtime_home).unwrap();
        crate::agents::persist_project_codex_runtime(&runtime).unwrap();
        let context = AgentContext {
            installation_id: runtime.runtime_installation_id,
            project_path: Some(
                std::fs::canonicalize(&project_b)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned(),
            ),
        };

        let error = inspect_project_codex_runtime(context).unwrap_err();

        assert!(error
            .message
            .contains("Project Codex runtime belongs to a different project"));
        match previous_home {
            Some(value) => std::env::set_var("AD_HOME", value),
            None => std::env::remove_var("AD_HOME"),
        }
        match previous_codex_home {
            Some(value) => std::env::set_var("CODEX_HOME", value),
            None => std::env::remove_var("CODEX_HOME"),
        }
    }

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
    #[serial_test::serial(home_env)]
    fn project_runtime_profile_apply_preserves_manifest_managed_state() {
        let temp = tempfile::tempdir().unwrap();
        let codex_home = temp.path().join(".codex");
        let project = temp.path().join("project");
        std::fs::create_dir_all(&codex_home).unwrap();
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(
            codex_home.join("config.toml"),
            "model = \"base\"\nfeature = \"base-feature\"\n",
        )
        .unwrap();
        let previous_home = std::env::var("AD_HOME").ok();
        let previous_codex_home = std::env::var("CODEX_HOME").ok();
        std::env::set_var("AD_HOME", temp.path());
        std::env::set_var("CODEX_HOME", &codex_home);

        let registry = builtin_registry();
        let base = registry
            .discover()
            .into_iter()
            .find(|item| item.agent_id.as_str() == "codex")
            .unwrap();
        let runtime = crate::agents::ProjectCodexRuntime::derive(&base, &project).unwrap();
        std::fs::create_dir_all(&runtime.runtime_home).unwrap();
        let initial_config = "model = \"project\"\n\n[plugins.\"demo@market\"]\nenabled = true\n";
        std::fs::write(runtime.runtime_home.join("config.toml"), initial_config).unwrap();
        let manifest = crate::agents::ProjectCodexRuntimeManifest {
            schema_version: crate::agents::PROJECT_CODEX_RUNTIME_MANIFEST_SCHEMA_VERSION,
            applied_inherit_base_config: true,
            applied_profile_id: None,
            project_overlay: crate::agents::ProjectPluginOverlay {
                marketplaces: Default::default(),
                enabled_plugins: std::collections::BTreeMap::from([("demo@market".into(), true)]),
            },
            project_settings_keys: std::collections::BTreeSet::from(["model".into()]),
        };
        std::fs::create_dir_all(runtime.manifest_path().parent().unwrap()).unwrap();
        std::fs::write(
            runtime.manifest_path(),
            crate::agents::render_project_codex_runtime_manifest(&manifest).unwrap(),
        )
        .unwrap();
        crate::agents::persist_project_codex_runtime(&runtime).unwrap();
        crate::agents::refresh_project_codex_runtime_digests(&runtime.runtime_installation_id)
            .unwrap();

        let now = chrono::Utc::now();
        crate::commands::profile_envelopes::save_profile_envelope(crate::agents::AgentProfile {
            schema_version: crate::agents::AGENT_PROFILE_SCHEMA_VERSION,
            key: crate::agents::AgentProfileKey {
                agent_id: AgentId::from("codex"),
                profile_id: crate::agents::ProfileId::from("project_api"),
            },
            metadata: crate::agents::ProfileMetadata {
                display_name: "Project API".into(),
                description: None,
                color: "#7C3AED".into(),
                created_at: now,
                updated_at: now,
            },
            payload_schema: crate::agents::CODEX_PROFILE_PAYLOAD_SCHEMA.into(),
            payload: serde_json::json!({
                "configToml": "model = \"profile\"\neffort = \"high\"\n"
            }),
        })
        .unwrap();
        let context = AgentContext {
            installation_id: runtime.runtime_installation_id.clone(),
            project_path: Some(runtime.project_path.clone()),
        };
        let store = PlanStore::default();

        let view = preview_agent_profile_apply_inner(
            context,
            crate::agents::ProfileId::from("project_api"),
            &store,
        )
        .unwrap();

        assert_eq!(view.changes.len(), 2);
        assert_eq!(view.changes[0].resource.logical_id, "runtime-manifest");
        assert_eq!(view.changes[1].resource.logical_id, "runtime-config");
        assert_eq!(
            std::fs::read_to_string(runtime.runtime_home.join("config.toml")).unwrap(),
            initial_config
        );

        crate::agents::ExecutionEngine
            .apply(&view.id, &store)
            .unwrap();
        let applied = std::fs::read_to_string(runtime.runtime_home.join("config.toml")).unwrap();
        assert!(applied.contains("model = \"profile\""));
        assert!(applied.contains("effort = \"high\""));
        assert!(applied.contains("feature = \"base-feature\""));
        assert!(applied.contains("[plugins.\"demo@market\"]"));
        let applied_manifest = crate::agents::load_project_codex_runtime_manifest(&runtime)
            .unwrap()
            .unwrap()
            .manifest;
        assert_eq!(
            applied_manifest.applied_profile_id.as_deref(),
            Some("project_api")
        );
        assert_eq!(
            applied_manifest.project_settings_keys,
            std::collections::BTreeSet::from(["effort".into(), "model".into()])
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
    fn settings_documents_exclude_native_project_config_targets() {
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

        assert_eq!(documents.len(), 1);
        let user_document = &documents[0];
        assert_eq!(
            user_document.resource.scope,
            crate::agents::ResourceScope::User
        );
        assert!(!user_document.exists);
        assert_eq!(
            user_document.content,
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
    fn settings_documents_accept_managed_project_runtime_symlink() {
        let temp = tempfile::tempdir().unwrap();
        let codex_home = temp.path().join(".codex");
        let project = temp.path().join("project");
        let legacy_runtime_home = temp.path().join(".ad/codex-homes/legacy-project");
        std::fs::create_dir_all(&codex_home).unwrap();
        std::fs::create_dir_all(&project).unwrap();
        std::fs::create_dir_all(&legacy_runtime_home).unwrap();
        std::fs::write(
            legacy_runtime_home.join("config.toml"),
            "model = \"runtime\"\n",
        )
        .unwrap();
        let previous_home = std::env::var("AD_HOME").ok();
        let previous_codex_home = std::env::var("CODEX_HOME").ok();
        std::env::set_var("AD_HOME", temp.path());
        std::env::set_var("CODEX_HOME", &codex_home);

        let base = builtin_registry()
            .discover()
            .into_iter()
            .find(|item| {
                item.agent_id.as_str() == "codex"
                    && item.root_path
                        == std::fs::canonicalize(&codex_home)
                            .unwrap()
                            .to_string_lossy()
            })
            .unwrap();
        let runtime = ProjectCodexRuntime::derive(&base, &project).unwrap();
        std::fs::create_dir_all(runtime.runtime_home.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&legacy_runtime_home, &runtime.runtime_home).unwrap();
        crate::agents::persist_project_codex_runtime(&runtime).unwrap();
        let context = AgentContext {
            installation_id: runtime.runtime_installation_id,
            project_path: Some(runtime.project_path),
        };

        let documents = list_agent_settings_documents(context).unwrap();

        assert_eq!(documents.len(), 1);
        assert_eq!(documents[0].resource.logical_id, "runtime-config");
        assert_eq!(
            documents[0].content,
            serde_json::Value::String("model = \"runtime\"\n".into())
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
        std::fs::write(
            operations.join("receipt-2.json"),
            serde_json::to_vec(&serde_json::json!({
                "id": "receipt-2",
                "planId": "plan-2",
                "status": "complete",
                "appliedResources": [{
                    "installationId": "codex:runtime",
                    "projectPath": "/Users/test/project",
                    "kind": "settings",
                    "scope": "project",
                    "logicalId": "runtime-config"
                }],
                "backupPaths": [],
                "postApplyStates": []
            }))
            .unwrap(),
        )
        .unwrap();
        std::fs::write(
            operations.join("receipt-3.json"),
            serde_json::to_vec(&serde_json::json!({
                "id": "receipt-3",
                "planId": "plan-3",
                "status": "complete",
                "appliedResources": [{
                    "installationId": "codex:default",
                    "projectPath": "/Users/test/other-project",
                    "kind": "settings",
                    "scope": "project",
                    "logicalId": "other-project-config"
                }],
                "backupPaths": [],
                "postApplyStates": []
            }))
            .unwrap(),
        )
        .unwrap();

        let entries = list_agent_operation_history(
            Some(InstallationId::from("codex:default")),
            None,
            Some(20),
        )
        .unwrap();
        let project_entries = list_agent_operation_history(
            Some(InstallationId::from("codex:default")),
            Some("/Users/test/project".into()),
            Some(20),
        )
        .unwrap();
        let other = list_agent_operation_history(
            Some(InstallationId::from("claude-code:default")),
            None,
            Some(20),
        )
        .unwrap();

        assert_eq!(entries.len(), 2);
        assert!(entries
            .iter()
            .any(|entry| entry.receipt.as_ref().unwrap().id.as_str() == "receipt-1"));
        assert!(entries
            .iter()
            .any(|entry| entry.receipt.as_ref().unwrap().id.as_str() == "receipt-3"));
        assert_eq!(project_entries.len(), 2);
        assert!(project_entries
            .iter()
            .all(|entry| entry.receipt.as_ref().unwrap().id.as_str() != "receipt-3"));
        assert!(other.is_empty());

        match previous_home {
            Some(value) => std::env::set_var("AD_HOME", value),
            None => std::env::remove_var("AD_HOME"),
        }
    }
}
