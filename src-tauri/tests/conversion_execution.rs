use ad_lib::agents::{
    builtin_registry, AgentContext, AgentErrorCode, ClaudeToCodexRoute, ConversionRoute,
    ExecutionEngine, OperationStatus, PlanStore, ResourceScope,
};
use serial_test::serial;

#[test]
#[serial(home_env)]
fn confirmed_conversion_applies_and_digest_protected_rollback_restores_target() {
    let home = tempfile::tempdir().unwrap();
    let claude_home = home.path().join(".claude");
    let codex_home = home.path().join(".codex");
    std::fs::create_dir_all(&claude_home).unwrap();
    std::fs::create_dir_all(&codex_home).unwrap();
    let source_bytes = include_bytes!("fixtures/conversion/claude-settings.json");
    let target_bytes = include_bytes!("fixtures/conversion/codex-config.toml");
    let source_path = claude_home.join("settings.json");
    let target_path = codex_home.join("config.toml");
    std::fs::write(&source_path, source_bytes).unwrap();
    std::fs::write(&target_path, target_bytes).unwrap();

    let previous_home = std::env::var("AD_HOME").ok();
    let previous_codex_home = std::env::var("CODEX_HOME").ok();
    std::env::set_var("AD_HOME", home.path());
    std::env::remove_var("CODEX_HOME");

    let (source, target) = contexts(None);
    let route = ClaudeToCodexRoute;
    let plans = PlanStore::default();
    let route_plan = route.preview(&source, &target).unwrap();
    let plan_id = route_plan.plan.id.clone();
    plans.insert_confirmation_required(route_plan.plan).unwrap();

    let unconfirmed = ExecutionEngine.apply(&plan_id, &plans).unwrap_err();
    assert_eq!(unconfirmed.code, AgentErrorCode::PermissionDenied);
    assert_eq!(std::fs::read(&target_path).unwrap(), target_bytes);

    let applied = ExecutionEngine.apply_confirmed(&plan_id, &plans).unwrap();
    assert_eq!(applied.status, OperationStatus::Complete);
    assert!(!applied.post_apply_states.is_empty());
    assert!(applied.manifest_digest.is_some());
    assert_eq!(std::fs::read(&source_path).unwrap(), source_bytes);
    assert_ne!(std::fs::read(&target_path).unwrap(), target_bytes);

    let rollback = ExecutionEngine.rollback(&applied.id).unwrap();
    assert_eq!(rollback.status, OperationStatus::Complete);
    assert_eq!(std::fs::read(&target_path).unwrap(), target_bytes);
    assert_eq!(std::fs::read(&source_path).unwrap(), source_bytes);

    let plans = PlanStore::default();
    let route_plan = route.preview(&source, &target).unwrap();
    let plan_id = route_plan.plan.id.clone();
    plans.insert_confirmation_required(route_plan.plan).unwrap();
    let applied = ExecutionEngine.apply_confirmed(&plan_id, &plans).unwrap();
    let external = b"model = \"externally-edited\"\n";
    std::fs::write(&target_path, external).unwrap();

    let error = ExecutionEngine.rollback(&applied.id).unwrap_err();

    restore_env(previous_home, previous_codex_home);
    assert_eq!(error.code, AgentErrorCode::ResourceChanged);
    assert_eq!(std::fs::read(&target_path).unwrap(), external);
    assert_eq!(std::fs::read(&source_path).unwrap(), source_bytes);
}

#[test]
#[serial(home_env)]
fn project_conversion_only_applies_and_rolls_back_project_scope() {
    let home = tempfile::tempdir().unwrap();
    let claude_home = home.path().join(".claude");
    let codex_home = home.path().join(".codex");
    let project = home.path().join("project");
    std::fs::create_dir_all(&claude_home).unwrap();
    std::fs::create_dir_all(&codex_home).unwrap();
    std::fs::create_dir_all(project.join(".claude")).unwrap();
    std::fs::create_dir_all(project.join(".codex")).unwrap();

    let user_source = include_bytes!("fixtures/conversion/claude-settings.json");
    let user_target = include_bytes!("fixtures/conversion/codex-config.toml");
    let shared_source = br#"{"model":"project-shared","model_reasoning_effort":"medium"}"#;
    let local_source = br#"{"model":"project-local","model_verbosity":"low"}"#;
    let project_target = b"project_only = true\n";
    let user_source_path = claude_home.join("settings.json");
    let user_target_path = codex_home.join("config.toml");
    let shared_source_path = project.join(".claude/settings.json");
    let local_source_path = project.join(".claude/settings.local.json");
    let project_target_path = project.join(".codex/config.toml");
    std::fs::write(&user_source_path, user_source).unwrap();
    std::fs::write(&user_target_path, user_target).unwrap();
    std::fs::write(&shared_source_path, shared_source).unwrap();
    std::fs::write(&local_source_path, local_source).unwrap();
    std::fs::write(&project_target_path, project_target).unwrap();

    let previous_home = std::env::var("AD_HOME").ok();
    let previous_codex_home = std::env::var("CODEX_HOME").ok();
    std::env::set_var("AD_HOME", home.path());
    std::env::remove_var("CODEX_HOME");

    let canonical_project = std::fs::canonicalize(&project)
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let (source, target) = contexts(Some(canonical_project));
    let route = ClaudeToCodexRoute;
    let plans = PlanStore::default();
    let route_plan = route.preview(&source, &target).unwrap();

    assert!(!route_plan.artifacts.is_empty());
    assert!(route_plan
        .artifacts
        .iter()
        .all(|artifact| artifact.source.scope == ResourceScope::Project));
    assert!(route_plan
        .plan
        .read_set
        .iter()
        .all(|precondition| precondition.resource.scope == ResourceScope::Project));
    assert!(route_plan
        .plan
        .mutations
        .iter()
        .all(|mutation| mutation.resource.scope == ResourceScope::Project));

    let plan_id = route_plan.plan.id.clone();
    plans.insert_confirmation_required(route_plan.plan).unwrap();
    let applied = ExecutionEngine.apply_confirmed(&plan_id, &plans).unwrap();

    assert_eq!(applied.status, OperationStatus::Complete);
    assert_eq!(std::fs::read(&user_source_path).unwrap(), user_source);
    assert_eq!(std::fs::read(&user_target_path).unwrap(), user_target);
    assert_eq!(std::fs::read(&shared_source_path).unwrap(), shared_source);
    assert_eq!(std::fs::read(&local_source_path).unwrap(), local_source);
    let converted = std::fs::read_to_string(&project_target_path).unwrap();
    assert!(converted.contains("model = \"project-local\""));
    assert!(converted.contains("model_reasoning_effort = \"medium\""));
    assert!(converted.contains("model_verbosity = \"low\""));
    assert!(converted.contains("project_only = true"));

    let rollback = ExecutionEngine.rollback(&applied.id).unwrap();

    restore_env(previous_home, previous_codex_home);
    assert_eq!(rollback.status, OperationStatus::Complete);
    assert_eq!(std::fs::read(&project_target_path).unwrap(), project_target);
    assert_eq!(std::fs::read(&user_target_path).unwrap(), user_target);
}

fn contexts(project_path: Option<String>) -> (AgentContext, AgentContext) {
    let installations = builtin_registry().discover();
    let context = |agent_id: &str| AgentContext {
        installation_id: installations
            .iter()
            .find(|installation| installation.agent_id.as_str() == agent_id)
            .unwrap()
            .id
            .clone(),
        project_path: project_path.clone(),
    };
    (context("claude-code"), context("codex"))
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
