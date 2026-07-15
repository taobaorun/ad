mod capabilities;
mod claude;
mod claude_ports;
mod codex;
mod codex_plugins;
mod codex_ports;
mod codex_runtime;
mod codex_skills;
mod conversion;
mod conversion_route;
mod discovery;
mod execution;
mod execution_fs;
#[cfg(test)]
mod execution_tests;
mod operations;
mod plan_store;
mod profiles;
mod registry;
mod runtime;
mod types;

pub use capabilities::*;
pub use claude::ClaudeAdapter;
pub use codex::CodexAdapter;
pub use conversion::{convert_claude_profile_to_codex, ArtifactDisposition, ConversionArtifact};
pub use conversion_route::*;
pub use discovery::*;
pub use execution::*;
pub use operations::*;
pub use plan_store::*;
pub use profiles::*;
pub use registry::*;
pub use runtime::*;
pub use types::*;

pub fn builtin_registry() -> AdapterRegistry {
    let mut registry = AdapterRegistry::new();
    registry.register(Box::new(ClaudeAdapter));
    registry.register(Box::new(CodexAdapter));
    registry
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_identity_includes_agent_id() {
        let claude = AgentProfileRef::new("claude-code", "default");
        let codex = AgentProfileRef::new("codex", "default");

        assert_ne!(claude, codex);
    }

    #[test]
    fn capability_round_trips_as_snake_case() {
        let json = serde_json::to_string(&Capability::ProcessDetection).unwrap();

        assert_eq!(json, "\"process_detection\"");
        assert_eq!(
            serde_json::from_str::<Capability>(&json).unwrap(),
            Capability::ProcessDetection
        );
    }

    #[test]
    #[serial_test::serial(home_env)]
    fn builtin_legacy_metadata_is_derived_from_v1_ports() {
        let temp = tempfile::tempdir().unwrap();
        let previous_home = std::env::var("AD_HOME").ok();
        let previous_codex_home = std::env::var("CODEX_HOME").ok();
        std::env::set_var("AD_HOME", temp.path());
        std::env::remove_var("CODEX_HOME");
        std::fs::create_dir_all(temp.path().join(".claude")).unwrap();
        std::fs::create_dir_all(temp.path().join(".codex")).unwrap();

        let registry = builtin_registry();
        let metadata = registry.metadata();
        let claude = metadata
            .iter()
            .find(|item| item.id.as_str() == "claude-code")
            .unwrap();
        let codex = metadata
            .iter()
            .find(|item| item.id.as_str() == "codex")
            .unwrap();

        assert_eq!(
            claude.capabilities.len(),
            registry
                .capability_descriptors("claude-code")
                .unwrap()
                .len()
        );
        assert_eq!(
            codex.capabilities.len(),
            registry.capability_descriptors("codex").unwrap().len()
        );
        assert_eq!(registry.discover().len(), 2);

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
