use crate::agents::{
    builtin_registry, convert_claude_profile_to_codex, AgentInstallation, AgentMetadata,
    ConversionPreview,
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
}
