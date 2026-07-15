use crate::fs::paths::claude_dir;

use super::claude_ports::{
    ClaudeLaunchPort, ClaudePluginsPort, ClaudeProcessPort, ClaudeSettingsPort, ClaudeSkillsPort,
};
use super::{
    AgentAdapter, AgentDefinition, ClaudeProfileSchema, DiscoveryEvidence, InstallationCandidate,
    LaunchPort, PluginsPort, ProcessPort, ProfileSchema, SettingsPort, SkillsPort,
};

#[derive(Debug, Default)]
pub struct ClaudeAdapter;

static SETTINGS_PORT: ClaudeSettingsPort = ClaudeSettingsPort;
static SKILLS_PORT: ClaudeSkillsPort = ClaudeSkillsPort;
static PLUGINS_PORT: ClaudePluginsPort = ClaudePluginsPort;
static PROCESS_PORT: ClaudeProcessPort = ClaudeProcessPort;
static LAUNCH_PORT: ClaudeLaunchPort = ClaudeLaunchPort;
static PROFILE_SCHEMA: ClaudeProfileSchema = ClaudeProfileSchema;

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

    fn skills(&self) -> Option<&dyn SkillsPort> {
        Some(&SKILLS_PORT)
    }

    fn plugins(&self) -> Option<&dyn PluginsPort> {
        Some(&PLUGINS_PORT)
    }

    fn processes(&self) -> Option<&dyn ProcessPort> {
        Some(&PROCESS_PORT)
    }

    fn launcher(&self) -> Option<&dyn LaunchPort> {
        Some(&LAUNCH_PORT)
    }

    fn profile_schema(&self) -> Option<&dyn ProfileSchema> {
        Some(&PROFILE_SCHEMA)
    }
}
