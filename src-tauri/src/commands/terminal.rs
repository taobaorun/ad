use std::path::PathBuf;

use serde::Serialize;

use crate::agents::{builtin_registry, AgentContext, AgentError, AgentErrorCode, LaunchRecipe};
use crate::terminal::{self, LaunchSpec, TerminalBackend};

#[derive(Serialize)]
pub struct TerminalBackendInfo {
    pub id: &'static str,
    pub label: &'static str,
}

#[tauri::command]
pub fn list_terminal_backends() -> Vec<TerminalBackendInfo> {
    vec![
        TerminalBackendInfo {
            id: "ghostty",
            label: TerminalBackend::Ghostty.label(),
        },
        TerminalBackendInfo {
            id: "cmux",
            label: TerminalBackend::Cmux.label(),
        },
        TerminalBackendInfo {
            id: "apple-terminal",
            label: TerminalBackend::AppleTerminal.label(),
        },
        TerminalBackendInfo {
            id: "custom",
            label: TerminalBackend::Custom.label(),
        },
    ]
}

#[tauri::command]
pub fn open_in_terminal(
    context: AgentContext,
    backend: String,
    custom_template: Option<String>,
) -> Result<(), AgentError> {
    let backend = TerminalBackend::from_str_lossy(&backend).map_err(|error| {
        terminal_error(&context, AgentErrorCode::InvalidPlan, error.to_string())
    })?;
    let recipe = resolve_launch_recipe(&context)?;
    let cwd = PathBuf::from(&recipe.cwd);
    let template = custom_template.as_deref().filter(|s| !s.is_empty());
    let spec = LaunchSpec {
        cwd: &cwd,
        program: &recipe.program,
        args: &recipe.args,
        env: &recipe.env,
        custom_template: template,
    };
    terminal::launch(backend, spec)
        .map_err(|error| terminal_error(&context, AgentErrorCode::Io, format!("{error:#}")))
}

fn resolve_launch_recipe(context: &AgentContext) -> Result<LaunchRecipe, AgentError> {
    let registry = builtin_registry();
    let installation = registry
        .discover()
        .into_iter()
        .find(|installation| installation.id == context.installation_id)
        .ok_or_else(|| {
            terminal_error(
                context,
                AgentErrorCode::Unsupported,
                "Unknown Agent installation",
            )
        })?;
    let adapter = registry
        .adapter(installation.agent_id.as_str())
        .ok_or_else(|| {
            terminal_error(
                context,
                AgentErrorCode::Unsupported,
                "Unknown Agent adapter",
            )
        })?;
    let launcher = adapter.launcher().ok_or_else(|| {
        terminal_error(
            context,
            AgentErrorCode::Unsupported,
            "Agent does not support terminal launch",
        )
    })?;
    launcher.recipe(context)
}

fn terminal_error(
    context: &AgentContext,
    code: AgentErrorCode,
    message: impl Into<String>,
) -> AgentError {
    AgentError {
        code,
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
    #[serial_test::serial(home_env)]
    fn resolves_terminal_recipe_from_the_selected_agent_adapter() {
        let temp = tempfile::tempdir().unwrap();
        let codex_home = temp.path().join(".codex");
        let project = temp.path().join("project");
        std::fs::create_dir_all(&codex_home).unwrap();
        std::fs::create_dir_all(&project).unwrap();
        let previous_home = std::env::var("AD_HOME").ok();
        let previous_codex_home = std::env::var("CODEX_HOME").ok();
        std::env::set_var("AD_HOME", temp.path());
        std::env::remove_var("CODEX_HOME");

        let installation = builtin_registry()
            .discover()
            .into_iter()
            .find(|item| item.agent_id.as_str() == "codex")
            .unwrap();
        let canonical_project = std::fs::canonicalize(project).unwrap();
        let context = AgentContext {
            installation_id: installation.id,
            project_path: Some(canonical_project.to_string_lossy().into_owned()),
        };
        let recipe = resolve_launch_recipe(&context).unwrap();

        match previous_home {
            Some(value) => std::env::set_var("AD_HOME", value),
            None => std::env::remove_var("AD_HOME"),
        }
        match previous_codex_home {
            Some(value) => std::env::set_var("CODEX_HOME", value),
            None => std::env::remove_var("CODEX_HOME"),
        }

        assert_eq!(recipe.program, "codex");
        assert_eq!(recipe.cwd, canonical_project.to_string_lossy());
    }
}
