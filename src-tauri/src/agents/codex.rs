use crate::fs::paths::codex_dir;

use super::{AgentAdapter, AgentDefinition, AgentInstallation};

#[derive(Debug, Default)]
pub struct CodexAdapter;

impl AgentAdapter for CodexAdapter {
    fn definition(&self) -> &AgentDefinition {
        static DEFINITION: std::sync::OnceLock<AgentDefinition> = std::sync::OnceLock::new();
        DEFINITION.get_or_init(|| AgentDefinition {
            id: "codex".into(),
            display_name: "Codex".into(),
            adapter_version: 1,
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
