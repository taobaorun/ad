use ad_lib::agents::{
    builtin_registry, AgentContext, AgentErrorCode, ClaudeToCodexRoute, ConversionRoute,
    ExecutionEngine, OperationStatus, PlanStore,
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

    let (source, target) = contexts();
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

fn contexts() -> (AgentContext, AgentContext) {
    let installations = builtin_registry().discover();
    let context = |agent_id: &str| AgentContext {
        installation_id: installations
            .iter()
            .find(|installation| installation.agent_id.as_str() == agent_id)
            .unwrap()
            .id
            .clone(),
        project_path: None,
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
