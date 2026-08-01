mod capabilities;
mod claude;
mod claude_ports;
mod codex;
mod codex_plugins;
mod codex_ports;
mod codex_runtime;
mod codex_skills;
mod collection_inventory;
mod conversion;
mod conversion_route;
mod discovery;
mod execution;
mod execution_confinement;
pub(crate) use execution_confinement::validate_project_workspace_root;
#[cfg(test)]
mod execution_confinement_tests;
mod execution_fs;
mod execution_journal;
mod execution_lock;
mod execution_recovery;
#[cfg(test)]
mod execution_recovery_tests;
mod execution_state;
#[cfg(test)]
mod execution_state_tests;
mod execution_targets;
#[cfg(test)]
mod execution_tests;
mod execution_tree;
mod operation_receipts;
mod operations;
mod plan_store;
mod plugin_conversion;
mod profiles;
mod project_codex_config;
mod project_codex_manifest;
mod project_codex_runtime;
mod project_inventory;
mod project_workspace;
mod registry;
mod resource_inventory;
mod resource_ownership;
mod runtime;
mod settings_inventory;
mod skill_activation;
mod skill_artifact_tree;
#[cfg(test)]
mod skill_artifact_tree_tests;
mod skill_artifacts;
#[cfg(test)]
mod skill_artifacts_tests;
mod types;
mod workspace_contracts;

pub use capabilities::*;
pub use claude::ClaudeAdapter;
pub use codex::CodexAdapter;
pub(crate) use codex_plugins::plan_project_runtime_profile_apply;
pub use conversion::{
    convert_claude_profile_to_codex, ArtifactDisposition, ConversionArtifact, ConversionEndpoint,
    ConversionResolutionKind, ConversionRiskLevel, ConversionSummary, ResolutionRequirement,
};
pub use conversion_route::*;
pub use discovery::*;
pub use execution::*;
pub use execution_fs::directory_tree_digest;
pub use execution_journal::*;
pub use execution_lock::*;
pub use execution_recovery::*;
pub use operation_receipts::*;
pub use operations::*;
pub use plan_store::*;
pub use plugin_conversion::*;
pub use profiles::*;
pub use project_codex_config::*;
pub use project_codex_manifest::*;
pub use project_codex_runtime::*;
pub use project_inventory::*;
pub use project_workspace::*;
pub use registry::*;
pub use resource_inventory::*;
pub use resource_ownership::*;
pub use runtime::*;
pub use skill_artifacts::*;
pub use types::*;
pub use workspace_contracts::*;

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
