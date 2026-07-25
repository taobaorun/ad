use crate::fs::paths::codex_dir;

use super::codex_plugins::CodexPluginsPort;
use super::codex_ports::CodexSettingsPort;
use super::codex_runtime::{CodexLaunchPort, CodexProcessPort};
use super::codex_skills::CodexSkillsPort;
use super::project_codex_runtime::discover_project_codex_candidates;
use super::{
    AgentAdapter, AgentDefinition, CodexProfileSchema, DiscoveryEvidence, InstallationCandidate,
    LaunchPort, PluginsPort, ProcessPort, ProfileSchema, SettingsPort, SkillsPort,
};

#[derive(Debug, Default)]
pub struct CodexAdapter;

static SETTINGS_PORT: CodexSettingsPort = CodexSettingsPort;
static SKILLS_PORT: CodexSkillsPort = CodexSkillsPort;
static PLUGINS_PORT: CodexPluginsPort = CodexPluginsPort;
static PROCESS_PORT: CodexProcessPort = CodexProcessPort;
static LAUNCH_PORT: CodexLaunchPort = CodexLaunchPort;
static PROFILE_SCHEMA: CodexProfileSchema = CodexProfileSchema;

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

    fn processes(&self) -> Option<&dyn ProcessPort> {
        Some(&PROCESS_PORT)
    }

    fn launcher(&self) -> Option<&dyn LaunchPort> {
        Some(&LAUNCH_PORT)
    }

    fn profile_schema(&self) -> Option<&dyn ProfileSchema> {
        Some(&PROFILE_SCHEMA)
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

    candidates.extend(discover_project_codex_candidates());

    candidates
}

#[cfg(test)]
mod tests {
    use super::*;

    fn restore_env(previous_home: Option<String>, previous_codex_home: Option<String>) {
        match previous_home {
            Some(value) => std::env::set_var("AD_HOME", value),
            None => std::env::remove_var("AD_HOME"),
        }
        match previous_codex_home {
            Some(value) => std::env::set_var("CODEX_HOME", value),
            None => std::env::remove_var("CODEX_HOME"),
        }
    }

    #[test]
    #[serial_test::serial(home_env)]
    fn runtime_ports_expose_codex_process_spec_and_launch_recipe() {
        let temp = tempfile::tempdir().unwrap();
        let codex_home = temp.path().join(".codex");
        let project = temp.path().join("project");
        std::fs::create_dir_all(&codex_home).unwrap();
        std::fs::create_dir_all(&project).unwrap();
        let previous_home = std::env::var("AD_HOME").ok();
        let previous_codex_home = std::env::var("CODEX_HOME").ok();
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
        let adapter = registry.adapter("codex").unwrap();
        let process_port = adapter.processes().unwrap();
        let process_spec = process_port.match_spec();
        let observations = process_port.detect(&context).unwrap();
        let recipe = adapter.launcher().unwrap().recipe(&context).unwrap();

        restore_env(previous_home, previous_codex_home);

        assert!(observations
            .iter()
            .all(|item| item.installation_id == context.installation_id));
        assert!(process_spec.matches("codex"));
        assert!(process_spec.matches("Codex-cli"));
        assert!(!process_spec.matches("claude"));
        assert_eq!(recipe.program, "codex");
        assert_eq!(
            recipe.cwd,
            std::fs::canonicalize(project).unwrap().to_string_lossy()
        );
        assert!(recipe.args.is_empty());
        assert!(recipe.env.is_empty());
    }

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
    fn native_project_config_is_inspect_only() {
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
        let error = port
            .plan_edit(
                &context,
                crate::agents::SettingsEdit {
                    resource: snapshot.resource,
                    media_type: "application/toml".into(),
                    content: serde_json::Value::String(
                        "model = \"project-new\"\nfuture = 1\n".into(),
                    ),
                },
            )
            .unwrap_err();

        assert_eq!(error.code, crate::agents::AgentErrorCode::Unsupported);
        assert!(port
            .edit_documents(&context)
            .unwrap()
            .iter()
            .all(|document| document.resource.logical_id != "project-config"));
        assert_eq!(
            std::fs::read_to_string(project_config).unwrap(),
            "model = \"project\"\nfuture = 1\n"
        );
        assert_eq!(
            std::fs::read_to_string(codex_home.join("config.toml")).unwrap(),
            "model = \"user\"\n"
        );
    }

    #[test]
    #[serial_test::serial(home_env)]
    fn project_runtime_settings_and_skill_toggle_stay_runtime_scoped() {
        let temp = tempfile::tempdir().unwrap();
        let codex_home = temp.path().join(".codex");
        let project = temp.path().join("project");
        let project_skill = project.join(".agents/skills/project-demo");
        std::fs::create_dir_all(&codex_home).unwrap();
        std::fs::create_dir_all(&project_skill).unwrap();
        std::fs::write(codex_home.join("config.toml"), "model = \"base\"\n").unwrap();
        std::fs::write(
            project_skill.join("SKILL.md"),
            "---\nname: project-demo\n---\n",
        )
        .unwrap();
        std::env::set_var("AD_HOME", temp.path());
        std::env::remove_var("CODEX_HOME");

        let registry = crate::agents::builtin_registry();
        let base = registry
            .discover()
            .into_iter()
            .find(|item| item.agent_id.as_str() == "codex")
            .unwrap();
        let runtime = crate::agents::ProjectCodexRuntime::derive(&base, &project).unwrap();
        std::fs::create_dir_all(&runtime.runtime_home).unwrap();
        std::fs::write(
            runtime.runtime_home.join("config.toml"),
            "model = \"project\"\ncli_auth_credentials_store = \"file\"\n\n[plugins.\"demo@market\"]\nenabled = true\n",
        )
        .unwrap();
        let manifest = crate::agents::ProjectCodexRuntimeManifest {
            schema_version: crate::agents::PROJECT_CODEX_RUNTIME_MANIFEST_SCHEMA_VERSION,
            applied_inherit_base_config: true,
            applied_profile_id: None,
            project_overlay: crate::agents::ProjectPluginOverlay {
                marketplaces: Default::default(),
                enabled_plugins: std::collections::BTreeMap::from([("demo@market".into(), true)]),
            },
            project_settings_keys: std::collections::BTreeSet::from(["model".into()]),
        };
        std::fs::create_dir_all(runtime.manifest_path().parent().unwrap()).unwrap();
        std::fs::write(
            runtime.manifest_path(),
            crate::agents::render_project_codex_runtime_manifest(&manifest).unwrap(),
        )
        .unwrap();
        crate::agents::persist_project_codex_runtime(&runtime).unwrap();
        crate::agents::refresh_project_codex_runtime_digests(&runtime.runtime_installation_id)
            .unwrap();
        let context = crate::agents::AgentContext {
            installation_id: runtime.runtime_installation_id.clone(),
            project_path: Some(runtime.project_path.clone()),
        };
        let adapter = registry.adapter("codex").unwrap();

        let documents = adapter
            .settings()
            .unwrap()
            .edit_documents(&context)
            .unwrap();
        assert_eq!(documents.len(), 1);
        assert_eq!(documents[0].resource.logical_id, "runtime-config");
        assert_eq!(
            documents[0].resource.scope,
            crate::agents::ResourceScope::Project
        );
        let settings_plan = adapter
            .settings()
            .unwrap()
            .plan_edit(
                &context,
                crate::agents::SettingsEdit {
                    resource: documents[0].resource.clone(),
                    media_type: "application/toml".into(),
                    content: serde_json::Value::String(
                        "model = \"project\"\neffort = \"high\"\ncli_auth_credentials_store = \"file\"\n\n[plugins.\"demo@market\"]\nenabled = true\n"
                            .into(),
                    ),
                },
            )
            .unwrap();
        assert_eq!(
            settings_plan.mutations[0].resource.logical_id,
            "runtime-manifest"
        );
        let settings_plan_id = settings_plan.id.clone();
        let settings_store = crate::agents::PlanStore::default();
        settings_store.insert(settings_plan).unwrap();
        crate::agents::ExecutionEngine
            .apply(&settings_plan_id, &settings_store)
            .unwrap();
        let settings_manifest = crate::agents::load_project_codex_runtime_manifest(&runtime)
            .unwrap()
            .unwrap()
            .manifest;
        assert!(settings_manifest.project_settings_keys.contains("model"));
        assert!(settings_manifest.project_settings_keys.contains("effort"));
        std::fs::write(
            codex_home.join("config.toml"),
            "model = \"base\"\nfeature = \"updated\"\n",
        )
        .unwrap();

        let skill = adapter
            .skills()
            .unwrap()
            .list(&context)
            .unwrap()
            .into_iter()
            .find(|snapshot| snapshot.resource.logical_id == "project-demo")
            .unwrap();
        let plan = adapter
            .skills()
            .unwrap()
            .plan_set_enabled(&context, &skill.resource, false)
            .unwrap();
        assert_eq!(plan.mutations[0].resource.logical_id, "runtime-manifest");
        assert_eq!(
            plan.mutations.last().unwrap().resource.logical_id,
            "runtime-config"
        );
        assert_eq!(
            plan.mutations.last().unwrap().resource.scope,
            crate::agents::ResourceScope::Project
        );
        assert_eq!(
            plan.mutations.last().unwrap().resource.project_path,
            context.project_path
        );
        let plan_id = plan.id.clone();
        let store = crate::agents::PlanStore::default();
        store.insert(plan).unwrap();
        let receipt = crate::agents::ExecutionEngine
            .apply(&plan_id, &store)
            .unwrap();

        assert_eq!(receipt.status, crate::agents::OperationStatus::Complete);
        let config = std::fs::read_to_string(runtime.runtime_home.join("config.toml")).unwrap();
        assert!(config.contains("[[skills.config]]"));
        assert!(config.contains("enabled = false"));
        let applied_manifest = crate::agents::load_project_codex_runtime_manifest(&runtime)
            .unwrap()
            .unwrap()
            .manifest;
        assert!(applied_manifest.project_settings_keys.contains("skills"));
        let stale_runtime =
            crate::agents::runtime_for_installation(&runtime.runtime_installation_id).unwrap();
        assert!(!crate::agents::project_codex_runtime_is_fresh(&stale_runtime).unwrap());

        let plugin = adapter
            .plugins()
            .unwrap()
            .list(&context)
            .unwrap()
            .into_iter()
            .find(|snapshot| snapshot.resource.logical_id == "demo@market")
            .unwrap();
        let plugin_plan = adapter
            .plugins()
            .unwrap()
            .plan_set_enabled(&context, &plugin.resource, false)
            .unwrap();
        let plugin_plan_id = plugin_plan.id.clone();
        let plugin_store = crate::agents::PlanStore::default();
        plugin_store.insert(plugin_plan).unwrap();
        crate::agents::ExecutionEngine
            .apply(&plugin_plan_id, &plugin_store)
            .unwrap();
        let rebuilt = std::fs::read_to_string(runtime.runtime_home.join("config.toml")).unwrap();
        assert!(rebuilt.contains("[[skills.config]]"));
        assert!(rebuilt.contains("enabled = false"));
        assert!(rebuilt.contains("effort = \"high\""));
        assert!(rebuilt.contains("feature = \"updated\""));
        let fresh_runtime =
            crate::agents::runtime_for_installation(&runtime.runtime_installation_id).unwrap();
        assert!(crate::agents::project_codex_runtime_is_fresh(&fresh_runtime).unwrap());
        assert_eq!(
            std::fs::read_to_string(codex_home.join("config.toml")).unwrap(),
            "model = \"base\"\nfeature = \"updated\"\n"
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
        std::fs::write(
            codex_home.join("config.toml"),
            "model = \"gpt-5.6\"\n\n[[skills.config]]\npath = \"../.agents/skills/user-demo/SKILL.md\"\nenabled = false\n",
        )
        .unwrap();
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
        let user_demo = snapshots
            .iter()
            .find(|item| item.resource.logical_id == "user-demo")
            .unwrap();
        assert_eq!(user_demo.content["enabled"], false);
        let enable_plan = port
            .plan_set_enabled(&context, &user_demo.resource, true)
            .unwrap();
        let enable_plan_id = enable_plan.id.clone();
        let enable_store = crate::agents::PlanStore::default();
        enable_store.insert(enable_plan).unwrap();
        crate::agents::ExecutionEngine
            .apply(&enable_plan_id, &enable_store)
            .unwrap();
        assert!(!std::fs::read_to_string(codex_home.join("config.toml"))
            .unwrap()
            .contains("user-demo"));
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
            "model = \"gpt-5.6\"\n\n[skills]\nconfig = []\n"
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
