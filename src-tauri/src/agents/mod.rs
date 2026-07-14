mod conversion;
mod claude;
mod codex;
mod registry;
mod types;

pub use claude::ClaudeAdapter;
pub use codex::CodexAdapter;
pub use conversion::convert_claude_profile_to_codex;
pub use registry::*;
pub use types::*;

pub fn builtin_registry() -> AdapterRegistry {
    let mut registry = AdapterRegistry::new();
    registry.register(Box::new(ClaudeAdapter::default()));
    registry.register(Box::new(CodexAdapter::default()));
    registry
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_installations_deduplicate_equivalent_paths() {
        let installations = deduplicate_installations(vec![
            AgentInstallation::new("codex", "/Users/test/.codex"),
            AgentInstallation::new("codex", "/Users/test/.codex/"),
        ]);

        assert_eq!(installations.len(), 1);
        assert_eq!(installations[0].root_path, "/Users/test/.codex");
    }

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
    fn builtins_expose_the_same_parity_capabilities() {
        let temp = tempfile::tempdir().unwrap();
        std::env::set_var("AD_HOME", temp.path());
        std::fs::create_dir_all(temp.path().join(".claude")).unwrap();
        std::fs::create_dir_all(temp.path().join(".codex")).unwrap();

        let registry = builtin_registry();
        let metadata = registry.metadata();
        let claude = metadata.iter().find(|item| item.id == "claude-code").unwrap();
        let codex = metadata.iter().find(|item| item.id == "codex").unwrap();

        assert_eq!(claude.capabilities, codex.capabilities);
        assert_eq!(registry.discover().len(), 2);

        std::env::remove_var("AD_HOME");
    }
}
