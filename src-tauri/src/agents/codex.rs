use crate::fs::paths::codex_dir;

use super::{AgentAdapter, AgentDefinition, DiscoveryEvidence, InstallationCandidate};

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

    fn discover(&self) -> Vec<InstallationCandidate> {
        let mut candidates = Vec::new();
        if let Ok(environment_home) = std::env::var("CODEX_HOME") {
            if let Some(candidate) = InstallationCandidate::from_existing_home(
                "codex",
                environment_home,
                DiscoveryEvidence::Environment,
            ) {
                candidates.push(candidate);
            }
        }

        if let Ok(default_home) = codex_dir() {
            if let Some(candidate) = InstallationCandidate::from_existing_home(
                "codex",
                default_home,
                DiscoveryEvidence::DefaultHome,
            ) {
                candidates.push(candidate);
            }
        }

        candidates
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[serial_test::serial(home_env)]
    fn discovery_keeps_distinct_default_and_environment_homes() {
        let temp = tempfile::tempdir().unwrap();
        let default_home = temp.path().join(".codex");
        let environment_home = temp.path().join("custom-codex");
        std::fs::create_dir_all(&default_home).unwrap();
        std::fs::create_dir_all(&environment_home).unwrap();

        let previous_home = std::env::var("AD_HOME").ok();
        let previous_codex_home = std::env::var("CODEX_HOME").ok();
        std::env::set_var("AD_HOME", temp.path());
        std::env::set_var("CODEX_HOME", &environment_home);

        let mut registry = crate::agents::AdapterRegistry::new();
        registry.register(Box::new(CodexAdapter));
        let installations = registry.discover();

        match previous_home {
            Some(value) => std::env::set_var("AD_HOME", value),
            None => std::env::remove_var("AD_HOME"),
        }
        match previous_codex_home {
            Some(value) => std::env::set_var("CODEX_HOME", value),
            None => std::env::remove_var("CODEX_HOME"),
        }

        assert_eq!(installations.len(), 2);
        assert_ne!(installations[0].id, installations[1].id);
    }

    #[test]
    #[serial_test::serial(home_env)]
    fn registry_deduplicates_trailing_codex_home_alias() {
        let temp = tempfile::tempdir().unwrap();
        let default_home = temp.path().join(".codex");
        std::fs::create_dir_all(&default_home).unwrap();

        let previous_home = std::env::var("AD_HOME").ok();
        let previous_codex_home = std::env::var("CODEX_HOME").ok();
        std::env::set_var("AD_HOME", temp.path());
        std::env::set_var("CODEX_HOME", format!("{}/", default_home.display()));

        let mut registry = crate::agents::AdapterRegistry::new();
        registry.register(Box::new(CodexAdapter));
        let installations = registry.discover();

        match previous_home {
            Some(value) => std::env::set_var("AD_HOME", value),
            None => std::env::remove_var("AD_HOME"),
        }
        match previous_codex_home {
            Some(value) => std::env::set_var("CODEX_HOME", value),
            None => std::env::remove_var("CODEX_HOME"),
        }

        assert_eq!(installations.len(), 1);
        assert_eq!(
            installations[0].root_path,
            std::fs::canonicalize(default_home)
                .unwrap()
                .to_string_lossy()
        );
    }
}
