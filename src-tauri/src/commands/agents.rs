use crate::agents::{
    builtin_registry, convert_claude_profile_to_codex, AgentContext, AgentInstallation,
    AgentMetadata, ConversionPreview, InstallationId,
};
use crate::models::ProfileFile;

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
}
