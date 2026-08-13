use ad_lib::agents::{
    builtin_registry, AgentContext, ArtifactDisposition, ClaudeToCodexOptions, ClaudeToCodexRoute,
    ExecutionEngine, PlanStore, ProjectCodexRuntime, ResourceKind,
};

fn write_json(path: &std::path::Path, value: serde_json::Value) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
}

#[test]
#[serial_test::serial(home_env)]
fn project_conversion_reports_plugins_unsupported_without_copying_or_transforming_them() {
    let temp = tempfile::tempdir().unwrap();
    let claude_home = temp.path().join(".claude");
    let codex_home = temp.path().join(".codex");
    let project = temp.path().join("project");
    let marketplace = claude_home.join("plugins/marketplaces/team");
    let package = claude_home.join("plugins/cache/team/alpha/1.0.0");
    std::fs::create_dir_all(marketplace.join(".agents/plugins")).unwrap();
    std::fs::create_dir_all(package.join(".codex-plugin")).unwrap();
    std::fs::create_dir_all(project.join(".claude")).unwrap();
    std::fs::create_dir_all(&codex_home).unwrap();
    write_json(
        &project.join(".claude/settings.json"),
        serde_json::json!({
            "model": "opus",
            "enabledPlugins": {"alpha@team": true}
        }),
    );
    write_json(
        &marketplace.join(".agents/plugins/marketplace.json"),
        serde_json::json!({"name": "team"}),
    );
    write_json(
        &package.join(".codex-plugin/plugin.json"),
        serde_json::json!({"name": "alpha", "version": "1.0.0"}),
    );
    std::fs::write(package.join("README.md"), "original plugin bytes\n").unwrap();
    let canonical_project = std::fs::canonicalize(&project).unwrap();
    write_json(
        &claude_home.join("plugins/installed_plugins.json"),
        serde_json::json!({
            "version": 2,
            "plugins": {
                "alpha@team": [{
                    "scope": "local",
                    "projectPath": canonical_project,
                    "installPath": package,
                    "version": "1.0.0",
                    "gitCommitSha": "abc123"
                }]
            }
        }),
    );
    write_json(
        &claude_home.join("plugins/known_marketplaces.json"),
        serde_json::json!({
            "team": {
                "source": {"source": "github", "repo": "acme/plugins"},
                "installLocation": marketplace
            }
        }),
    );
    std::fs::write(
        codex_home.join("config.toml"),
        "model = \"gpt-5.6\"\ncli_auth_credentials_store = \"file\"\n",
    )
    .unwrap();
    std::fs::write(codex_home.join("auth.json"), "shared-login").unwrap();
    let source_before = std::fs::read(package.join("README.md")).unwrap();

    let previous_home = std::env::var("AD_HOME").ok();
    let previous_codex_home = std::env::var("CODEX_HOME").ok();
    std::env::set_var("AD_HOME", temp.path());
    std::env::remove_var("CODEX_HOME");

    let registry = builtin_registry();
    let installations = registry.discover();
    let claude = installations
        .iter()
        .find(|installation| installation.agent_id.as_str() == "claude-code")
        .unwrap();
    let codex = installations
        .iter()
        .find(|installation| installation.agent_id.as_str() == "codex")
        .unwrap();
    let runtime = ProjectCodexRuntime::derive(codex, &project).unwrap();
    let source_context = AgentContext {
        installation_id: claude.id.clone(),
        project_path: Some(runtime.project_path.clone()),
    };
    let target_context = AgentContext {
        installation_id: runtime.runtime_installation_id.clone(),
        project_path: Some(runtime.project_path.clone()),
    };

    let preview = ClaudeToCodexRoute
        .preview_with_options(
            &source_context,
            &target_context,
            &ClaudeToCodexOptions {
                target_model: Some("project-model".into()),
                ..ClaudeToCodexOptions::default()
            },
        )
        .unwrap();
    let plugin = preview
        .artifacts
        .iter()
        .find(|artifact| artifact.id == "plugin:alpha@team")
        .unwrap();
    assert_eq!(plugin.disposition, ArtifactDisposition::Unsupported);
    assert_eq!(
        plugin.detail_code.as_deref(),
        Some("unsupported_agent_capability")
    );
    assert!(preview.plan.mutations.iter().all(|mutation| {
        mutation.resource.kind != ResourceKind::Plugins
            || matches!(
                mutation.resource.logical_id.as_str(),
                "runtime-auth" | "runtime-config" | "runtime-manifest"
            )
    }));
    assert!(!preview.plan.mutations.iter().any(|mutation| {
        mutation.resource.logical_id.starts_with("marketplace:")
            || mutation.resource.logical_id.starts_with("package:")
    }));

    let plan_id = preview.plan.id.clone();
    let plans = PlanStore::default();
    plans.insert(preview.plan).unwrap();
    ExecutionEngine.apply(&plan_id, &plans).unwrap();

    assert_eq!(
        std::fs::read(package.join("README.md")).unwrap(),
        source_before
    );
    assert!(!runtime.runtime_home.join("plugins/cache").exists());
    assert!(!runtime.runtime_home.join(".tmp/marketplaces").exists());

    match previous_home {
        Some(value) => std::env::set_var("AD_HOME", value),
        None => std::env::remove_var("AD_HOME"),
    }
    match previous_codex_home {
        Some(value) => std::env::set_var("CODEX_HOME", value),
        None => std::env::remove_var("CODEX_HOME"),
    }
}
