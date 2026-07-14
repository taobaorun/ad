use std::collections::BTreeSet;

use crate::fs::paths::claude_dir;

use super::{AgentAdapter, AgentInstallation, AgentMetadata, Capability};

#[derive(Debug, Default)]
pub struct ClaudeAdapter;

impl AgentAdapter for ClaudeAdapter {
    fn metadata(&self) -> &AgentMetadata {
        static METADATA: std::sync::OnceLock<AgentMetadata> = std::sync::OnceLock::new();
        METADATA.get_or_init(|| AgentMetadata {
            id: "claude-code".into(),
            display_name: "Claude Code".into(),
            capabilities: BTreeSet::from([
                Capability::Settings,
                Capability::Skills,
                Capability::Plugins,
                Capability::ProcessDetection,
                Capability::TerminalLaunch,
                Capability::Conversion,
            ]),
        })
    }

    fn discover(&self) -> Vec<AgentInstallation> {
        let Ok(root) = claude_dir() else {
            return Vec::new();
        };
        if root.is_dir() {
            vec![AgentInstallation::new("claude-code", root.to_string_lossy())]
        } else {
            Vec::new()
        }
    }
}

