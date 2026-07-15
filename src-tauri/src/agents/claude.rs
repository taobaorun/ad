use crate::fs::paths::claude_dir;

use super::claude_ports::ClaudeSettingsPort;
use super::{
    AgentAdapter, AgentDefinition, DiscoveryEvidence, InstallationCandidate, SettingsPort,
};

#[derive(Debug, Default)]
pub struct ClaudeAdapter;

static SETTINGS_PORT: ClaudeSettingsPort = ClaudeSettingsPort;

impl AgentAdapter for ClaudeAdapter {
    fn definition(&self) -> &AgentDefinition {
        static DEFINITION: std::sync::OnceLock<AgentDefinition> = std::sync::OnceLock::new();
        DEFINITION.get_or_init(|| AgentDefinition {
            id: "claude-code".into(),
            display_name: "Claude Code".into(),
            adapter_version: 1,
        })
    }

    fn discover(&self) -> Vec<InstallationCandidate> {
        let Ok(root) = claude_dir() else {
            return Vec::new();
        };
        InstallationCandidate::from_existing_home(
            "claude-code",
            root,
            DiscoveryEvidence::DefaultHome,
        )
        .into_iter()
        .collect()
    }

    fn settings(&self) -> Option<&dyn SettingsPort> {
        Some(&SETTINGS_PORT)
    }
}
