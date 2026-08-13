use ad_lib::agents::{
    builtin_registry, persist_project_codex_runtime, AgentContext, AgentErrorCode,
    CapabilityOperation, CollectionInstallRequest, ProjectCodexRuntime,
};

fn project_context(
    temp: &tempfile::TempDir,
) -> (
    ad_lib::agents::AdapterRegistry,
    AgentContext,
    ProjectCodexRuntime,
) {
    let base_home = temp.path().join(".codex");
    let project = temp.path().join("project");
    std::fs::create_dir_all(&base_home).unwrap();
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(
        base_home.join("config.toml"),
        "cli_auth_credentials_store = \"file\"\n",
    )
    .unwrap();
    std::fs::write(base_home.join("auth.json"), "shared-user-login").unwrap();
    std::env::set_var("AD_HOME", temp.path());
    std::env::remove_var("CODEX_HOME");

    let registry = builtin_registry();
    let base = registry
        .discover()
        .into_iter()
        .find(|installation| {
            installation.root_path == std::fs::canonicalize(&base_home).unwrap().to_string_lossy()
        })
        .unwrap();
    let runtime = ProjectCodexRuntime::derive(&base, &project).unwrap();
    std::fs::create_dir_all(&runtime.runtime_home).unwrap();
    persist_project_codex_runtime(&runtime).unwrap();
    let context = AgentContext {
        installation_id: runtime.runtime_installation_id.clone(),
        project_path: Some(runtime.project_path.clone()),
    };
    (registry, context, runtime)
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

#[test]
#[serial_test::serial(home_env)]
fn codex_plugin_capability_is_read_only_for_new_installation() {
    let temp = tempfile::tempdir().unwrap();
    let previous_home = std::env::var("AD_HOME").ok();
    let previous_codex_home = std::env::var("CODEX_HOME").ok();
    let (registry, context, _runtime) = project_context(&temp);
    let port = registry.adapter("codex").unwrap().plugins().unwrap();

    let error = port
        .plan_install(
            &context,
            CollectionInstallRequest {
                logical_id: String::new(),
                source: serde_json::json!({"catalogResourceId": "resource:plugin"}),
            },
        )
        .unwrap_err();

    assert_eq!(error.code, AgentErrorCode::Unsupported);
    assert!(error.message.contains("unsupported_agent_capability"));
    assert!(!port.operations().contains(&CapabilityOperation::Install));
    restore_env(previous_home, previous_codex_home);
}

#[test]
#[serial_test::serial(home_env)]
fn codex_plugin_adapter_rejects_legacy_staged_install_without_writing_runtime() {
    let temp = tempfile::tempdir().unwrap();
    let previous_home = std::env::var("AD_HOME").ok();
    let previous_codex_home = std::env::var("CODEX_HOME").ok();
    let (registry, context, runtime) = project_context(&temp);
    let stage = temp
        .path()
        .join(".ad/staging/codex-plugin-conversion/legacy");
    std::fs::create_dir_all(stage.join("marketplace")).unwrap();
    std::fs::create_dir_all(stage.join("package")).unwrap();
    std::fs::write(stage.join("package/original.txt"), "unchanged").unwrap();
    let source_before = std::fs::read(stage.join("package/original.txt")).unwrap();

    let error = registry
        .adapter("codex")
        .unwrap()
        .plugins()
        .unwrap()
        .plan_install(
            &context,
            CollectionInstallRequest {
                logical_id: "demo@team".into(),
                source: serde_json::json!({
                    "marketplace": {"stagePath": stage.join("marketplace")},
                    "package": {"stagePath": stage.join("package")}
                }),
            },
        )
        .unwrap_err();

    assert_eq!(error.code, AgentErrorCode::Unsupported);
    assert!(error.message.contains("unsupported_agent_capability"));
    assert_eq!(
        std::fs::read(stage.join("package/original.txt")).unwrap(),
        source_before
    );
    assert!(!runtime.runtime_home.join("plugins").exists());
    assert!(!runtime.runtime_home.join(".tmp/marketplaces").exists());
    assert!(!runtime.runtime_home.join("config.toml").exists());
    restore_env(previous_home, previous_codex_home);
}
