use crate::agents::{builtin_registry, AgentInstallation, AgentMetadata};

use super::CmdResult;

#[tauri::command]
pub fn list_agents() -> CmdResult<Vec<AgentMetadata>> {
    Ok(builtin_registry().metadata())
}

#[tauri::command]
pub fn discover_agents() -> CmdResult<Vec<AgentInstallation>> {
    Ok(builtin_registry().discover())
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
