use std::path::PathBuf;

use serde::Serialize;

use crate::agents::{
    builtin_registry, project_codex_runtime_is_fresh, project_runtime_descriptor_for_base_project,
    project_runtime_for_base_project, runtime_for_installation, AgentContext, AgentError,
    AgentErrorCode, LaunchRecipe,
};
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
    let effective_context = if installation.agent_id.as_str() == "codex" {
        project_codex_launch_context(context)?
    } else {
        context.clone()
    };
    let installation = registry
        .discover()
        .into_iter()
        .find(|installation| installation.id == effective_context.installation_id)
        .ok_or_else(|| {
            terminal_error(
                &effective_context,
                AgentErrorCode::Unsupported,
                "Project Codex runtime is not registered",
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
    launcher.recipe(&effective_context)
}

fn project_codex_launch_context(context: &AgentContext) -> Result<AgentContext, AgentError> {
    let Some(project_path) = context.project_path.as_deref() else {
        return Ok(context.clone());
    };
    let desired_inherit_base_config = super::projects::load()
        .map_err(|error| terminal_error(context, AgentErrorCode::Io, error.to_string()))?
        .into_iter()
        .find(|project| project.path == project_path)
        .map(|project| project.inherit_base_config)
        .unwrap_or(true);
    let registered = runtime_for_installation(&context.installation_id).or_else(|| {
        project_runtime_for_base_project(
            &context.installation_id,
            std::path::Path::new(project_path),
        )
    });
    let runtime = match registered {
        Some(runtime) => Some(runtime),
        None => project_runtime_descriptor_for_base_project(
            &context.installation_id,
            std::path::Path::new(project_path),
        )
        .map_err(|error| terminal_error(context, AgentErrorCode::Io, error.to_string()))?,
    };
    let Some(runtime) = runtime else {
        return Ok(context.clone());
    };
    if runtime.generated_config_digest.is_none()
        && !runtime.runtime_home.join("config.toml").is_file()
    {
        if !desired_inherit_base_config {
            return Err(terminal_error(
                context,
                AgentErrorCode::ResourceChanged,
                "Project Codex runtime policy needs Preview and Apply before launch",
            ));
        }
        return Ok(context.clone());
    }
    if desired_inherit_base_config != runtime.applied_inherit_base_config {
        return Err(terminal_error(
            context,
            AgentErrorCode::ResourceChanged,
            "Project Codex runtime policy needs Preview and Apply before launch",
        ));
    }
    if !project_codex_runtime_is_fresh(&runtime)
        .map_err(|error| terminal_error(context, AgentErrorCode::Io, error.to_string()))?
    {
        return Err(terminal_error(
            context,
            AgentErrorCode::ResourceChanged,
            "Project Codex runtime needs Preview and Apply before launch",
        ));
    }
    Ok(AgentContext {
        installation_id: runtime.runtime_installation_id,
        project_path: Some(runtime.project_path),
    })
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

    #[test]
    #[serial_test::serial(home_env)]
    fn unprepared_project_runtime_does_not_override_base_codex_launch() {
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
        let runtime = crate::agents::ProjectCodexRuntime::derive(&base, &project).unwrap();
        std::fs::create_dir_all(&runtime.runtime_home).unwrap();
        crate::agents::persist_project_codex_runtime(&runtime).unwrap();
        let context = AgentContext {
            installation_id: base.id,
            project_path: Some(runtime.project_path.clone()),
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

        assert!(!recipe.env.contains_key("CODEX_HOME"));
        assert!(recipe.args.is_empty());
        assert_eq!(recipe.cwd, runtime.project_path);
    }

    #[test]
    #[serial_test::serial(home_env)]
    fn isolated_project_without_an_applied_runtime_fails_closed() {
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
        let canonical_project = std::fs::canonicalize(project).unwrap();
        let project_path = canonical_project.to_string_lossy().into_owned();
        crate::commands::projects::add_project(project_path.clone()).unwrap();
        crate::commands::projects::set_project_codex_config_inheritance(
            project_path.clone(),
            false,
        )
        .unwrap();
        let context = AgentContext {
            installation_id: base.id,
            project_path: Some(project_path),
        };

        let error = resolve_launch_recipe(&context).unwrap_err();

        match previous_home {
            Some(value) => std::env::set_var("AD_HOME", value),
            None => std::env::remove_var("AD_HOME"),
        }
        match previous_codex_home {
            Some(value) => std::env::set_var("CODEX_HOME", value),
            None => std::env::remove_var("CODEX_HOME"),
        }

        assert_eq!(error.code, AgentErrorCode::ResourceChanged);
    }

    #[test]
    #[serial_test::serial(home_env)]
    fn unregistered_generated_runtime_never_falls_back_to_base() {
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
        let runtime = crate::agents::ProjectCodexRuntime::derive(&base, &project).unwrap();
        std::fs::create_dir_all(&runtime.runtime_home).unwrap();
        std::fs::write(
            runtime.runtime_home.join("config.toml"),
            "cli_auth_credentials_store = \"file\"\n",
        )
        .unwrap();
        let context = AgentContext {
            installation_id: base.id,
            project_path: Some(runtime.project_path),
        };

        let error = resolve_launch_recipe(&context).unwrap_err();

        match previous_home {
            Some(value) => std::env::set_var("AD_HOME", value),
            None => std::env::remove_var("AD_HOME"),
        }
        match previous_codex_home {
            Some(value) => std::env::set_var("CODEX_HOME", value),
            None => std::env::remove_var("CODEX_HOME"),
        }

        assert_eq!(error.code, AgentErrorCode::ResourceChanged);
    }

    #[test]
    #[serial_test::serial(home_env)]
    fn project_launch_uses_a_fresh_derived_codex_home_and_blocks_stale_base_config() {
        let temp = tempfile::tempdir().unwrap();
        let codex_home = temp.path().join(".codex");
        let project = temp.path().join("project");
        std::fs::create_dir_all(&codex_home).unwrap();
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(codex_home.join("config.toml"), "model = \"base\"\n").unwrap();
        let previous_home = std::env::var("AD_HOME").ok();
        let previous_codex_home = std::env::var("CODEX_HOME").ok();
        std::env::set_var("AD_HOME", temp.path());
        std::env::remove_var("CODEX_HOME");
        let base = builtin_registry()
            .discover()
            .into_iter()
            .find(|installation| installation.agent_id.as_str() == "codex")
            .unwrap();
        let mut runtime = crate::agents::ProjectCodexRuntime::derive(&base, &project).unwrap();
        runtime.profile_id = Some("project-api".into());
        std::fs::create_dir_all(&runtime.runtime_home).unwrap();
        std::fs::write(
            runtime.runtime_home.join("config.toml"),
            "model = \"base\"\ncli_auth_credentials_store = \"file\"\n",
        )
        .unwrap();
        crate::agents::persist_project_codex_runtime(&runtime).unwrap();
        crate::agents::refresh_project_codex_runtime_digests(&runtime.runtime_installation_id)
            .unwrap();
        let context = AgentContext {
            installation_id: base.id,
            project_path: Some(runtime.project_path.clone()),
        };

        let recipe = resolve_launch_recipe(&context).unwrap();
        assert_eq!(
            recipe.env.get("CODEX_HOME").map(String::as_str),
            Some(runtime.runtime_home.to_string_lossy().as_ref())
        );
        assert_eq!(recipe.args, vec!["--profile", "project-api"]);

        crate::commands::projects::add_project(runtime.project_path.clone()).unwrap();
        crate::commands::projects::set_project_codex_config_inheritance(
            runtime.project_path.clone(),
            false,
        )
        .unwrap();
        let error = resolve_launch_recipe(&context).unwrap_err();
        assert_eq!(error.code, AgentErrorCode::ResourceChanged);
        crate::commands::projects::set_project_codex_config_inheritance(
            runtime.project_path.clone(),
            true,
        )
        .unwrap();

        std::fs::write(codex_home.join("config.toml"), "model = \"changed\"\n").unwrap();
        let error = resolve_launch_recipe(&context).unwrap_err();
        assert_eq!(error.code, AgentErrorCode::ResourceChanged);

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
