use crate::fs::paths::codex_dir;

use super::codex_ports::CodexSettingsPort;
use super::{
    AgentAdapter, AgentDefinition, DiscoveryEvidence, InstallationCandidate, SettingsPort,
};

#[derive(Debug, Default)]
pub struct CodexAdapter;

static SETTINGS_PORT: CodexSettingsPort = CodexSettingsPort;

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
        discover_codex_candidates()
    }

    fn settings(&self) -> Option<&dyn SettingsPort> {
        Some(&SETTINGS_PORT)
    }
}

pub(crate) fn discover_codex_candidates() -> Vec<InstallationCandidate> {
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

    #[test]
    #[serial_test::serial(home_env)]
    fn settings_port_preserves_unknown_toml_without_touching_sensitive_state() {
        let temp = tempfile::tempdir().unwrap();
        let codex_home = temp.path().join(".codex");
        std::fs::create_dir_all(&codex_home).unwrap();
        let original = concat!(
            "model = \"gpt-5.4\"\n",
            "unknown_future_key = true\n\n",
            "[mcp_servers.demo]\n",
            "command = \"demo\"\n",
        );
        std::fs::write(codex_home.join("config.toml"), original).unwrap();
        std::fs::write(codex_home.join("auth.json"), "do-not-read").unwrap();
        std::fs::write(codex_home.join("history.jsonl"), "do-not-read").unwrap();
        std::env::set_var("AD_HOME", temp.path());
        std::env::remove_var("CODEX_HOME");

        let registry = crate::agents::builtin_registry();
        let installation = registry
            .discover()
            .into_iter()
            .find(|item| item.agent_id.as_str() == "codex")
            .unwrap();
        let context = crate::agents::AgentContext {
            installation_id: installation.id,
            project_path: None,
        };
        let port = registry.adapter("codex").unwrap().settings().unwrap();

        let snapshots = port.inspect(&context).unwrap();
        assert_eq!(snapshots.len(), 1);
        assert_eq!(snapshots[0].content.as_str(), Some(original));
        assert_eq!(snapshots[0].resource.logical_id, "user-config");
        let edited = original.replace("gpt-5.4", "gpt-5.6");
        let plan = port
            .plan_edit(
                &context,
                crate::agents::SettingsEdit {
                    resource: snapshots[0].resource.clone(),
                    media_type: "application/toml".into(),
                    content: serde_json::Value::String(edited.clone()),
                },
            )
            .unwrap();

        assert_eq!(plan.mutations[0].content.as_ref().unwrap(), &edited);
        assert_eq!(
            std::fs::read_to_string(codex_home.join("config.toml")).unwrap(),
            original
        );
        let mut auth = snapshots[0].resource.clone();
        auth.logical_id = "auth.json".into();
        assert_eq!(
            port.resolve(&context, &auth).unwrap_err().code,
            crate::agents::AgentErrorCode::InvalidPlan
        );
    }

    #[test]
    #[serial_test::serial(home_env)]
    fn project_settings_plan_applies_through_the_shared_engine() {
        let temp = tempfile::tempdir().unwrap();
        let codex_home = temp.path().join(".codex");
        let project = temp.path().join("project");
        std::fs::create_dir_all(&codex_home).unwrap();
        std::fs::create_dir_all(project.join(".codex")).unwrap();
        std::fs::write(codex_home.join("config.toml"), "model = \"user\"\n").unwrap();
        let project_config = project.join(".codex/config.toml");
        std::fs::write(&project_config, "model = \"project\"\nfuture = 1\n").unwrap();
        std::env::set_var("AD_HOME", temp.path());
        std::env::remove_var("CODEX_HOME");

        let registry = crate::agents::builtin_registry();
        let installation = registry
            .discover()
            .into_iter()
            .find(|item| item.agent_id.as_str() == "codex")
            .unwrap();
        let context = crate::agents::AgentContext {
            installation_id: installation.id,
            project_path: Some(
                std::fs::canonicalize(&project)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned(),
            ),
        };
        let port = registry.adapter("codex").unwrap().settings().unwrap();
        let snapshot = port
            .inspect(&context)
            .unwrap()
            .into_iter()
            .find(|snapshot| snapshot.resource.scope == crate::agents::ResourceScope::Project)
            .unwrap();
        let updated = "model = \"project-new\"\nfuture = 1\n";
        let plan = port
            .plan_edit(
                &context,
                crate::agents::SettingsEdit {
                    resource: snapshot.resource,
                    media_type: "application/toml".into(),
                    content: serde_json::Value::String(updated.into()),
                },
            )
            .unwrap();
        let plan_id = plan.id.clone();
        let store = crate::agents::PlanStore::default();
        store.insert(plan).unwrap();

        let receipt = crate::agents::ExecutionEngine
            .apply(&plan_id, &store)
            .unwrap();

        assert_eq!(receipt.status, crate::agents::OperationStatus::Complete);
        assert_eq!(std::fs::read_to_string(project_config).unwrap(), updated);
        assert_eq!(
            std::fs::read_to_string(codex_home.join("config.toml")).unwrap(),
            "model = \"user\"\n"
        );
    }
}
