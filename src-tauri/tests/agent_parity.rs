use std::collections::BTreeSet;
use std::path::Path;

use ad_lib::agents::{
    builtin_registry, AgentAdapter, AgentContext, AgentErrorCode, CapabilityAvailability,
    CapabilityOperation, CollectionInstallRequest, ExecutionEngine, MutationPlan, OperationStatus,
    PlanStore, ResourceScope, SettingsEdit,
};

struct ContractExpectation {
    agent_id: &'static str,
    program: &'static str,
    process_names: &'static [&'static str],
    settings_media_type: &'static str,
    settings_content: serde_json::Value,
    skills_availability: CapabilityAvailability,
}

#[test]
#[serial_test::serial(home_env)]
fn claude_and_codex_satisfy_the_same_required_user_journeys() {
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("project");
    let skill_source = temp.path().join(".ad/skill-library/parity/parity-skill");
    create_fixtures(temp.path(), &project, &skill_source);

    let previous_home = std::env::var("AD_HOME").ok();
    let previous_codex_home = std::env::var("CODEX_HOME").ok();
    std::env::set_var("AD_HOME", temp.path());
    std::env::remove_var("CODEX_HOME");

    let registry = builtin_registry();
    let project_path = std::fs::canonicalize(&project)
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let claude_context = context_for(&registry, "claude-code", &project_path);
    let codex_context = context_for(&registry, "codex", &project_path);

    assert_required_journeys(
        registry.adapter("claude-code").unwrap(),
        &claude_context,
        &skill_source,
        ContractExpectation {
            agent_id: "claude-code",
            program: "claude",
            process_names: &["claude", "claude-code"],
            settings_media_type: "application/json",
            settings_content: serde_json::json!({
                "model": "claude-sonnet-4-5",
                "enabledPlugins": {"demo": true}
            }),
            skills_availability: CapabilityAvailability::Degraded,
        },
    );
    assert_required_journeys(
        registry.adapter("codex").unwrap(),
        &codex_context,
        &skill_source,
        ContractExpectation {
            agent_id: "codex",
            program: "codex",
            process_names: &["codex", "codex-cli"],
            settings_media_type: "application/toml",
            settings_content: serde_json::Value::String(
                "model = \"gpt-5.4\"\n\n[plugins.demo]\nenabled = true\n".into(),
            ),
            skills_availability: CapabilityAvailability::Available,
        },
    );
    assert_codex_snapshots_exclude_sensitive_runtime_files(
        registry.adapter("codex").unwrap(),
        &codex_context,
    );

    restore_env(previous_home, previous_codex_home);
}

fn assert_codex_snapshots_exclude_sensitive_runtime_files(
    adapter: &dyn AgentAdapter,
    context: &AgentContext,
) {
    let snapshots = adapter
        .settings()
        .unwrap()
        .inspect(context)
        .unwrap()
        .into_iter()
        .chain(adapter.skills().unwrap().list(context).unwrap())
        .chain(adapter.plugins().unwrap().list(context).unwrap())
        .collect::<Vec<_>>();

    assert!(!snapshots.is_empty());
    for snapshot in snapshots {
        assert!(!snapshot.location.path.ends_with("auth.json"));
        assert!(!snapshot.location.path.contains("/history"));
        assert!(!snapshot.location.path.contains("/sessions/"));
        assert!(!snapshot.location.path.contains("/logs/"));
    }
}

fn assert_required_journeys(
    adapter: &dyn AgentAdapter,
    context: &AgentContext,
    skill_source: &Path,
    expected: ContractExpectation,
) {
    assert_eq!(adapter.definition().id.as_str(), expected.agent_id);

    let settings = adapter.settings().expect("settings port");
    assert_eq!(settings.availability(), CapabilityAvailability::Available);
    assert!(!settings
        .operations()
        .contains(&CapabilityOperation::Rollback));
    assert_operations(
        settings.operations(),
        &[
            CapabilityOperation::Inspect,
            CapabilityOperation::Edit,
            CapabilityOperation::Preview,
            CapabilityOperation::Apply,
        ],
    );
    let settings_snapshot = settings
        .inspect(context)
        .unwrap()
        .into_iter()
        .find(|snapshot| snapshot.resource.scope == ResourceScope::User)
        .expect("user settings snapshot");
    let settings_plan = settings
        .plan_edit(
            context,
            SettingsEdit {
                resource: settings_snapshot.resource,
                media_type: expected.settings_media_type.into(),
                content: expected.settings_content,
            },
        )
        .unwrap();
    assert_eq!(settings_plan.agent_id.as_str(), expected.agent_id);
    assert_eq!(settings_plan.mutations.len(), 1);
    apply_complete(settings_plan);

    let skills = adapter.skills().expect("skills port");
    assert_eq!(skills.availability(), expected.skills_availability);
    assert!(!skills.operations().contains(&CapabilityOperation::Rollback));
    if skills.availability() == CapabilityAvailability::Degraded {
        assert!(!skills.limitations().is_empty());
    }
    assert_operations(
        skills.operations(),
        &[
            CapabilityOperation::List,
            CapabilityOperation::Install,
            CapabilityOperation::Enable,
            CapabilityOperation::Disable,
            CapabilityOperation::Preview,
            CapabilityOperation::Apply,
        ],
    );
    skills.list(context).unwrap();
    let install_plan = skills
        .plan_install(
            context,
            CollectionInstallRequest {
                logical_id: "parity-skill".into(),
                source: serde_json::json!({"path": skill_source}),
            },
        )
        .unwrap();
    let skill_resource = install_plan.mutations[0].resource.clone();
    apply_complete(install_plan);
    let disable_plan = skills
        .plan_set_enabled(context, &skill_resource, false)
        .unwrap();
    assert!(!disable_plan.mutations.is_empty());
    apply_complete(disable_plan);

    let plugins = adapter.plugins().expect("plugins port");
    assert_eq!(plugins.availability(), CapabilityAvailability::Degraded);
    assert!(!plugins
        .operations()
        .contains(&CapabilityOperation::Rollback));
    assert!(!plugins.limitations().is_empty());
    assert_operations(
        plugins.operations(),
        &[
            CapabilityOperation::List,
            CapabilityOperation::Enable,
            CapabilityOperation::Disable,
            CapabilityOperation::Preview,
            CapabilityOperation::Apply,
        ],
    );
    let plugin = plugins
        .list(context)
        .unwrap()
        .into_iter()
        .find(|snapshot| snapshot.resource.logical_id == "demo")
        .expect("demo plugin snapshot");
    let plugin_plan = plugins
        .plan_set_enabled(context, &plugin.resource, false)
        .unwrap();
    assert_eq!(plugin_plan.mutations.len(), 1);
    apply_complete(plugin_plan);
    let install_error = plugins
        .plan_install(
            context,
            CollectionInstallRequest {
                logical_id: "demo".into(),
                source: serde_json::json!({}),
            },
        )
        .unwrap_err();
    assert_eq!(install_error.code, AgentErrorCode::Unsupported);

    let processes = adapter.processes().expect("process port");
    assert_eq!(processes.availability(), CapabilityAvailability::Available);
    assert_operations(processes.operations(), &[CapabilityOperation::Detect]);
    let match_spec = processes.match_spec();
    for name in expected.process_names {
        assert!(match_spec.matches(name), "missing process name {name}");
    }
    assert!(processes
        .detect(context)
        .unwrap()
        .iter()
        .all(|process| process.installation_id == context.installation_id));

    let launcher = adapter.launcher().expect("launch port");
    assert_eq!(launcher.availability(), CapabilityAvailability::Available);
    assert_operations(launcher.operations(), &[CapabilityOperation::Launch]);
    let recipe = launcher.recipe(context).unwrap();
    assert_eq!(recipe.program, expected.program);
    assert_eq!(recipe.cwd, context.project_path.as_deref().unwrap());
}

fn assert_operations(actual: BTreeSet<CapabilityOperation>, required: &[CapabilityOperation]) {
    for operation in required {
        assert!(
            actual.contains(operation),
            "missing operation {operation:?}"
        );
    }
}

fn apply_complete(plan: MutationPlan) {
    let plan_id = plan.id.clone();
    let plans = PlanStore::default();
    plans.insert(plan).unwrap();
    let receipt = ExecutionEngine.apply(&plan_id, &plans).unwrap();
    assert_eq!(receipt.status, OperationStatus::Complete);
}

fn context_for(
    registry: &ad_lib::agents::AdapterRegistry,
    agent_id: &str,
    project_path: &str,
) -> AgentContext {
    let installation = registry
        .discover()
        .into_iter()
        .find(|installation| installation.agent_id.as_str() == agent_id)
        .unwrap();
    AgentContext {
        installation_id: installation.id,
        project_path: Some(project_path.into()),
    }
}

fn create_fixtures(home: &Path, project: &Path, skill_source: &Path) {
    std::fs::create_dir_all(home.join(".claude")).unwrap();
    std::fs::create_dir_all(home.join(".codex")).unwrap();
    std::fs::create_dir_all(project.join(".claude")).unwrap();
    std::fs::create_dir_all(project.join(".codex")).unwrap();
    std::fs::create_dir_all(home.join(".codex/sessions")).unwrap();
    std::fs::create_dir_all(home.join(".codex/logs")).unwrap();
    std::fs::create_dir_all(skill_source).unwrap();
    std::fs::write(
        home.join(".claude/settings.json"),
        r#"{"model":"claude-opus-4-1","enabledPlugins":{"demo":true}}"#,
    )
    .unwrap();
    std::fs::write(
        home.join(".codex/config.toml"),
        "model = \"gpt-5.3\"\n\n[plugins.demo]\nenabled = true\n",
    )
    .unwrap();
    std::fs::write(home.join(".codex/auth.json"), "fixture").unwrap();
    std::fs::write(home.join(".codex/history.jsonl"), "history").unwrap();
    std::fs::write(home.join(".codex/sessions/session.json"), "session").unwrap();
    std::fs::write(home.join(".codex/logs/codex.log"), "log").unwrap();
    std::fs::write(project.join(".claude/settings.json"), "{}").unwrap();
    std::fs::write(project.join(".claude/settings.local.json"), "{}").unwrap();
    std::fs::write(project.join(".codex/config.toml"), "model = \"project\"\n").unwrap();
    std::fs::write(
        skill_source.join("SKILL.md"),
        "---\nname: parity-skill\ndescription: Parity fixture\n---\n",
    )
    .unwrap();
}

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
