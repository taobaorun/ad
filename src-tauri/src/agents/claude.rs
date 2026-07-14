use crate::fs::paths::claude_dir;

use super::{AgentAdapter, AgentDefinition, AgentInstallation};

#[derive(Debug, Default)]
pub struct ClaudeAdapter;

impl AgentAdapter for ClaudeAdapter {
    fn definition(&self) -> &AgentDefinition {
        static DEFINITION: std::sync::OnceLock<AgentDefinition> = std::sync::OnceLock::new();
        DEFINITION.get_or_init(|| AgentDefinition {
            id: "claude-code".into(),
            display_name: "Claude Code".into(),
            adapter_version: 1,
        })
    }

    fn discover(&self) -> Vec<AgentInstallation> {
        let Ok(root) = claude_dir() else {
            return Vec::new();
        };
        if root.is_dir() {
            vec![AgentInstallation::new(
                "claude-code",
                root.to_string_lossy(),
            )]
        } else {
            Vec::new()
        }
    }
}
