use ad_lib::agents::{
    builtin_registry, persist_project_codex_runtime, resolve_project_agent_workspace,
    AgentErrorCode, ProjectCodexRuntime,
};

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
fn claude_project_workspace_has_a_stable_canonical_key() {
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("project");
    let project_alias = temp.path().join("project-alias");
    std::fs::create_dir_all(temp.path().join(".claude")).unwrap();
    std::fs::create_dir_all(&project).unwrap();
    std::os::unix::fs::symlink(&project, &project_alias).unwrap();
    let previous_home = std::env::var("AD_HOME").ok();
    let previous_codex_home = std::env::var("CODEX_HOME").ok();
    std::env::set_var("AD_HOME", temp.path());
    std::env::remove_var("CODEX_HOME");

    let installation = builtin_registry()
        .discover()
        .into_iter()
        .find(|installation| installation.agent_id.as_str() == "claude-code")
        .unwrap();
    let direct = resolve_project_agent_workspace(&installation.id, &project).unwrap();
    let alias = resolve_project_agent_workspace(&installation.id, &project_alias).unwrap();

    restore_env(previous_home, previous_codex_home);

    assert_eq!(direct, alias);
    assert_eq!(direct.base_installation_id, installation.id);
    assert_eq!(direct.effective_installation_id, installation.id);
    assert!(direct.project_runtime.is_none());
    assert_eq!(
        direct.canonical_project_path,
        std::fs::canonicalize(project).unwrap().to_string_lossy()
    );
}

#[test]
#[serial_test::serial(home_env)]
fn prepared_codex_runtime_converges_base_and_runtime_contexts() {
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("project");
    std::fs::create_dir_all(temp.path().join(".codex")).unwrap();
    std::fs::create_dir_all(&project).unwrap();
    let previous_home = std::env::var("AD_HOME").ok();
    let previous_codex_home = std::env::var("CODEX_HOME").ok();
    std::env::set_var("AD_HOME", temp.path());
    std::env::remove_var("CODEX_HOME");

    let base = builtin_registry()
        .discover()
        .into_iter()
        .find(|installation| installation.agent_id.as_str() == "codex")
        .unwrap();
    let runtime = ProjectCodexRuntime::derive(&base, &project).unwrap();
    std::fs::create_dir_all(&runtime.runtime_home).unwrap();
    std::fs::write(
        runtime.runtime_home.join("config.toml"),
        "model = \"gpt-5\"\n",
    )
    .unwrap();
    persist_project_codex_runtime(&runtime).unwrap();

    let base_workspace = resolve_project_agent_workspace(&base.id, &project).unwrap();
    let runtime_workspace =
        resolve_project_agent_workspace(&runtime.runtime_installation_id, &project).unwrap();

    restore_env(previous_home, previous_codex_home);

    assert_eq!(base_workspace, runtime_workspace);
    assert_eq!(base_workspace.base_installation_id, base.id);
    assert_eq!(
        base_workspace.effective_installation_id,
        runtime.runtime_installation_id
    );
    let identity = base_workspace.project_runtime.unwrap();
    assert_eq!(identity.base_installation_id, base.id);
    assert_eq!(identity.installation_id, runtime.runtime_installation_id);
    assert!(identity
        .revision
        .as_str()
        .starts_with("runtime-revision:sha256:"));
}

#[test]
#[serial_test::serial(home_env)]
fn same_basename_projects_have_distinct_workspace_keys() {
    let temp = tempfile::tempdir().unwrap();
    let first = temp.path().join("first/project");
    let second = temp.path().join("second/project");
    std::fs::create_dir_all(temp.path().join(".claude")).unwrap();
    std::fs::create_dir_all(&first).unwrap();
    std::fs::create_dir_all(&second).unwrap();
    let previous_home = std::env::var("AD_HOME").ok();
    let previous_codex_home = std::env::var("CODEX_HOME").ok();
    std::env::set_var("AD_HOME", temp.path());
    std::env::remove_var("CODEX_HOME");

    let installation = builtin_registry()
        .discover()
        .into_iter()
        .find(|installation| installation.agent_id.as_str() == "claude-code")
        .unwrap();
    let first = resolve_project_agent_workspace(&installation.id, &first).unwrap();
    let second = resolve_project_agent_workspace(&installation.id, &second).unwrap();

    restore_env(previous_home, previous_codex_home);

    assert_ne!(first.key, second.key);
    assert_ne!(first.canonical_project_path, second.canonical_project_path);
}

#[test]
#[serial_test::serial(home_env)]
fn workspace_resolution_rejects_unknown_and_mismatched_installations() {
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("project");
    let other_project = temp.path().join("other-project");
    std::fs::create_dir_all(temp.path().join(".codex")).unwrap();
    std::fs::create_dir_all(&project).unwrap();
    std::fs::create_dir_all(&other_project).unwrap();
    let previous_home = std::env::var("AD_HOME").ok();
    let previous_codex_home = std::env::var("CODEX_HOME").ok();
    std::env::set_var("AD_HOME", temp.path());
    std::env::remove_var("CODEX_HOME");

    let base = builtin_registry()
        .discover()
        .into_iter()
        .find(|installation| installation.agent_id.as_str() == "codex")
        .unwrap();
    let runtime = ProjectCodexRuntime::derive(&base, &project).unwrap();
    std::fs::create_dir_all(&runtime.runtime_home).unwrap();
    std::fs::write(
        runtime.runtime_home.join("config.toml"),
        "model = \"gpt-5\"\n",
    )
    .unwrap();
    persist_project_codex_runtime(&runtime).unwrap();

    let unknown = resolve_project_agent_workspace(&"codex:missing".into(), &project).unwrap_err();
    let mismatch =
        resolve_project_agent_workspace(&runtime.runtime_installation_id, &other_project)
            .unwrap_err();

    restore_env(previous_home, previous_codex_home);

    assert_eq!(unknown.code, AgentErrorCode::InvalidPlan);
    assert_eq!(mismatch.code, AgentErrorCode::InvalidPlan);
    assert!(mismatch.message.contains("different project"));
}
