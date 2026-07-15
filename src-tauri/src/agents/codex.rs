use crate::fs::paths::codex_dir;

use super::codex_plugins::CodexPluginsPort;
use super::codex_ports::CodexSettingsPort;
use super::codex_skills::CodexSkillsPort;
use super::{
    AgentAdapter, AgentDefinition, DiscoveryEvidence, InstallationCandidate, PluginsPort,
    SettingsPort, SkillsPort,
};

#[derive(Debug, Default)]
pub struct CodexAdapter;

static SETTINGS_PORT: CodexSettingsPort = CodexSettingsPort;
static SKILLS_PORT: CodexSkillsPort = CodexSkillsPort;
static PLUGINS_PORT: CodexPluginsPort = CodexPluginsPort;

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

    fn skills(&self) -> Option<&dyn SkillsPort> {
        Some(&SKILLS_PORT)
    }

    fn plugins(&self) -> Option<&dyn PluginsPort> {
        Some(&PLUGINS_PORT)
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

    #[test]
    #[serial_test::serial(home_env)]
    fn skills_port_lists_scopes_and_plans_without_writing() {
        let temp = tempfile::tempdir().unwrap();
        let codex_home = temp.path().join(".codex");
        let user_skill = temp.path().join(".agents/skills/user-demo");
        let project = temp.path().join("project");
        let project_skill = project.join(".agents/skills/project-demo");
        std::fs::create_dir_all(&codex_home).unwrap();
        std::fs::create_dir_all(&user_skill).unwrap();
        std::fs::create_dir_all(&project_skill).unwrap();
        std::fs::write(codex_home.join("config.toml"), "model = \"gpt-5.6\"\n").unwrap();
        std::fs::write(user_skill.join("SKILL.md"), "---\nname: user-demo\n---\n").unwrap();
        std::fs::write(
            project_skill.join("SKILL.md"),
            "---\nname: project-demo\n---\n",
        )
        .unwrap();
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
        let port = registry.adapter("codex").unwrap().skills().unwrap();

        let snapshots = port.list(&context).unwrap();
        assert!(snapshots
            .iter()
            .any(|item| item.resource.logical_id == "user-demo"));
        let project_demo = snapshots
            .iter()
            .find(|item| item.resource.logical_id == "project-demo")
            .unwrap();
        let plan = port
            .plan_set_enabled(&context, &project_demo.resource, false)
            .unwrap();

        assert_eq!(
            plan.mutations[0].resource.kind,
            crate::agents::ResourceKind::Settings
        );
        assert_eq!(
            std::fs::read_to_string(codex_home.join("config.toml")).unwrap(),
            "model = \"gpt-5.6\"\n"
        );
        let plan_id = plan.id.clone();
        let store = crate::agents::PlanStore::default();
        store.insert(plan).unwrap();
        crate::agents::ExecutionEngine
            .apply(&plan_id, &store)
            .unwrap();
        let config = std::fs::read_to_string(codex_home.join("config.toml")).unwrap();
        assert!(config.contains("[[skills.config]]"));
        assert!(config.contains("enabled = false"));

        let source = temp.path().join("source/install-demo");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::write(source.join("SKILL.md"), "---\nname: install-demo\n---\n").unwrap();
        let install = port
            .plan_install(
                &context,
                crate::agents::CollectionInstallRequest {
                    logical_id: "install-demo".into(),
                    source: serde_json::json!({"path": source}),
                },
            )
            .unwrap();
        let install_id = install.id.clone();
        let install_store = crate::agents::PlanStore::default();
        install_store.insert(install).unwrap();
        crate::agents::ExecutionEngine
            .apply(&install_id, &install_store)
            .unwrap();
        assert!(project.join(".agents/skills/install-demo").is_symlink());
    }

    #[test]
    #[serial_test::serial(home_env)]
    fn plugins_port_lists_and_plans_toggle_but_reports_install_limitation() {
        let temp = tempfile::tempdir().unwrap();
        let codex_home = temp.path().join(".codex");
        std::fs::create_dir_all(&codex_home).unwrap();
        std::fs::write(
            codex_home.join("config.toml"),
            "future = 1\n\n[plugins.\"demo@market\"]\nenabled = true\n",
        )
        .unwrap();
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
        let port = registry.adapter("codex").unwrap().plugins().unwrap();

        let plugin = port.list(&context).unwrap().remove(0);
        let plan = port
            .plan_set_enabled(&context, &plugin.resource, false)
            .unwrap();
        let install_error = port
            .plan_install(
                &context,
                crate::agents::CollectionInstallRequest {
                    logical_id: "demo@market".into(),
                    source: serde_json::json!({}),
                },
            )
            .unwrap_err();

        assert_eq!(
            plan.mutations[0].resource.kind,
            crate::agents::ResourceKind::Plugins
        );
        assert_eq!(
            install_error.code,
            crate::agents::AgentErrorCode::Unsupported
        );
        assert_eq!(
            port.availability(),
            crate::agents::CapabilityAvailability::Degraded
        );
        assert!(!port.limitations().is_empty());
        let plan_id = plan.id.clone();
        let store = crate::agents::PlanStore::default();
        store.insert(plan).unwrap();
        crate::agents::ExecutionEngine
            .apply(&plan_id, &store)
            .unwrap();
        let updated = std::fs::read_to_string(codex_home.join("config.toml")).unwrap();
        let parsed = updated.parse::<toml::Value>().unwrap();
        assert_eq!(
            parsed["plugins"]["demo@market"]["enabled"].as_bool(),
            Some(false)
        );
        assert_eq!(parsed["future"].as_integer(), Some(1));
    }
}
