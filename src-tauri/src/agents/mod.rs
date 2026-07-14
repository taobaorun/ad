mod types;

pub use types::*;

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
}
