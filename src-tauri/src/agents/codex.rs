use std::collections::BTreeSet;

use crate::fs::paths::codex_dir;

use super::{AgentAdapter, AgentInstallation, AgentMetadata, Capability};

#[derive(Debug, Default)]
pub struct CodexAdapter;

impl AgentAdapter for CodexAdapter {
    fn metadata(&self) -> &AgentMetadata {
        static METADATA: std::sync::OnceLock<AgentMetadata> = std::sync::OnceLock::new();
        METADATA.get_or_init(|| AgentMetadata {
            id: "codex".into(),
            display_name: "Codex".into(),
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
        let Ok(root) = codex_dir() else {
            return Vec::new();
        };
        if root.is_dir() {
            vec![AgentInstallation::new("codex", root.to_string_lossy())]
        } else {
            Vec::new()
        }
    }
}

