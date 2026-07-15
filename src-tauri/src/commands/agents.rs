use crate::agents::{
    builtin_registry, convert_claude_profile_to_codex, AgentContext, AgentError, AgentErrorCode,
    AgentInstallation, AgentMetadata, ConversionPreview, ExecutionEngine, InstallationId,
    MutationPlanView, OperationReceipt, PlanId, PlanStore, SettingsEdit,
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
pub fn preview_agent_settings_edit(
    context: AgentContext,
    edit: SettingsEdit,
    plans: State<'_, PlanStore>,
) -> Result<MutationPlanView, AgentError> {
    preview_agent_settings_edit_inner(context, edit, plans.inner())
}

#[tauri::command]
pub fn apply_agent_plan(
    plan_id: PlanId,
    plans: State<'_, PlanStore>,
) -> Result<OperationReceipt, AgentError> {
    ExecutionEngine.apply(&plan_id, plans.inner())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_built_in_agents() {
        let agents = list_agents().unwrap();

        assert_eq!(agents.iter().map(|agent| agent.id.as_str()).collect::<Vec<_>>(), [
            "claude-code",
            "codex",
        ]);
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
        assert_eq!(context.project_path.as_deref(), Some(expected_project.as_str()));
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
}
