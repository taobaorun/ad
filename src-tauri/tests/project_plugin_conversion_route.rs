use ad_lib::agents::{
    builtin_registry, load_project_codex_runtime_manifest, persist_project_codex_runtime,
    AgentContext, ArtifactDisposition, ClaudeToCodexOptions, ClaudeToCodexRoute, ExecutionEngine,
    PlanStore, ProjectCodexRuntime,
};

fn write_json(path: &std::path::Path, value: serde_json::Value) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
}

#[test]
#[serial_test::serial(home_env)]
fn project_route_bootstraps_runtime_without_convertible_artifacts() {
    let temp = tempfile::tempdir().unwrap();
    let claude_home = temp.path().join(".claude");
    let codex_home = temp.path().join(".codex");
    let project = temp.path().join("project");
    std::fs::create_dir_all(&claude_home).unwrap();
    std::fs::create_dir_all(&codex_home).unwrap();
    std::fs::create_dir_all(project.join(".claude")).unwrap();
    std::fs::create_dir_all(project.join(".codex")).unwrap();
    write_json(
        &project.join(".claude/settings.json"),
        serde_json::json!({"model": "claude-project-model"}),
    );
    let native_project_config = b"model = \"native-project-model\"\n";
    std::fs::write(project.join(".codex/config.toml"), native_project_config).unwrap();
    std::fs::write(
        codex_home.join("config.toml"),
        "model = \"gpt-5.6\"\ncli_auth_credentials_store = \"file\"\n",
    )
    .unwrap();
    std::fs::write(codex_home.join("auth.json"), "shared-login").unwrap();
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
    let base = installations
        .iter()
        .find(|installation| installation.agent_id.as_str() == "codex")
        .unwrap();
    let runtime = ProjectCodexRuntime::derive(base, &project).unwrap();
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

    assert!(!runtime.runtime_home.exists());
    assert!(!runtime.state_path().unwrap().exists());
    assert!(!runtime.runtime_home.join("config.toml").exists());
    assert!(preview
        .plan
        .mutations
        .iter()
        .any(|mutation| mutation.resource.logical_id == "runtime-auth"));
    assert!(preview
        .plan
        .mutations
        .iter()
        .any(|mutation| mutation.resource.logical_id == "runtime-config"));

    let plan_id = preview.plan.id.clone();
    let plans = PlanStore::default();
    plans.insert(preview.plan).unwrap();
    ExecutionEngine.apply(&plan_id, &plans).unwrap();

    assert!(runtime.runtime_home.is_dir());
    let applied_runtime =
        ad_lib::agents::refresh_project_codex_runtime_digests(&runtime.runtime_installation_id)
            .unwrap()
            .unwrap();
    assert!(applied_runtime.generated_config_digest.is_some());
    assert!(applied_runtime.manifest_digest.is_some());
    let config = std::fs::read_to_string(runtime.runtime_home.join("config.toml"))
        .unwrap()
        .parse::<toml::Value>()
        .unwrap();
    assert_eq!(config["model"].as_str(), Some("project-model"));
    assert_eq!(config["cli_auth_credentials_store"].as_str(), Some("file"));
    assert_eq!(
        std::fs::read_link(runtime.runtime_home.join("auth.json")).unwrap(),
        std::fs::canonicalize(codex_home.join("auth.json")).unwrap()
    );
    assert_eq!(
        std::fs::read(project.join(".codex/config.toml")).unwrap(),
        native_project_config
    );

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
fn isolated_project_route_excludes_base_config_but_keeps_shared_auth() {
    let temp = tempfile::tempdir().unwrap();
    let claude_home = temp.path().join(".claude");
    let codex_home = temp.path().join(".codex");
    let project = temp.path().join("project");
    std::fs::create_dir_all(&claude_home).unwrap();
    std::fs::create_dir_all(&codex_home).unwrap();
    std::fs::create_dir_all(project.join(".claude")).unwrap();
    write_json(
        &project.join(".claude/settings.json"),
        serde_json::json!({"model": "claude-project-model"}),
    );
    std::fs::write(
        codex_home.join("config.toml"),
        concat!(
            "model = \"base-model\"\n",
            "cli_auth_credentials_store = \"file\"\n\n",
            "[mcp_servers.base]\n",
            "command = \"base-mcp\"\n\n",
            "[plugins.\"base@shared\"]\n",
            "enabled = true\n",
        ),
    )
    .unwrap();
    std::fs::write(codex_home.join("auth.json"), "shared-login").unwrap();
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
    let base = installations
        .iter()
        .find(|installation| installation.agent_id.as_str() == "codex")
        .unwrap();
    let runtime = ProjectCodexRuntime::derive(base, &project).unwrap();
    std::fs::create_dir_all(&runtime.runtime_home).unwrap();
    std::fs::write(
        runtime.runtime_home.join("config.toml"),
        concat!(
            "model = \"base-model\"\n",
            "cli_auth_credentials_store = \"file\"\n\n",
            "[mcp_servers.base]\n",
            "command = \"base-mcp\"\n\n",
            "[plugins.\"base@shared\"]\n",
            "enabled = true\n",
        ),
    )
    .unwrap();
    persist_project_codex_runtime(&runtime).unwrap();
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
                target_model: Some("base-model".into()),
                inherit_base_config: false,
                ..ClaudeToCodexOptions::default()
            },
        )
        .unwrap();

    assert!(preview
        .plan
        .mutations
        .iter()
        .any(|mutation| mutation.resource.logical_id == "runtime-manifest"));
    let plan_id = preview.plan.id.clone();
    let plans = PlanStore::default();
    plans.insert(preview.plan).unwrap();
    ExecutionEngine.apply(&plan_id, &plans).unwrap();

    let config = std::fs::read_to_string(runtime.runtime_home.join("config.toml"))
        .unwrap()
        .parse::<toml::Value>()
        .unwrap();
    assert_eq!(
        config.get("model").and_then(toml::Value::as_str),
        Some("base-model")
    );
    assert!(config.get("mcp_servers").is_none());
    assert!(config.get("plugins").is_none());
    assert_eq!(
        std::fs::read_link(runtime.runtime_home.join("auth.json")).unwrap(),
        std::fs::canonicalize(codex_home.join("auth.json")).unwrap()
    );
    let manifest = load_project_codex_runtime_manifest(&runtime)
        .unwrap()
        .unwrap()
        .manifest;
    assert!(!manifest.applied_inherit_base_config);
    assert!(manifest.applied_profile_id.is_none());
    assert!(manifest.project_overlay.marketplaces.is_empty());
    assert!(manifest.project_overlay.enabled_plugins.is_empty());

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
fn legacy_runtime_with_unproven_plugin_ownership_fails_closed() {
    let temp = tempfile::tempdir().unwrap();
    let claude_home = temp.path().join(".claude");
    let codex_home = temp.path().join(".codex");
    let project = temp.path().join("project");
    std::fs::create_dir_all(&claude_home).unwrap();
    std::fs::create_dir_all(&codex_home).unwrap();
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(
        codex_home.join("config.toml"),
        "cli_auth_credentials_store = \"file\"\n",
    )
    .unwrap();
    std::fs::write(codex_home.join("auth.json"), "shared-login").unwrap();
    let previous_home = std::env::var("AD_HOME").ok();
    let previous_codex_home = std::env::var("CODEX_HOME").ok();
    std::env::set_var("AD_HOME", temp.path());
    std::env::remove_var("CODEX_HOME");

    let installations = builtin_registry().discover();
    let claude = installations
        .iter()
        .find(|installation| installation.agent_id.as_str() == "claude-code")
        .unwrap();
    let base = installations
        .iter()
        .find(|installation| installation.agent_id.as_str() == "codex")
        .unwrap();
    let runtime = ProjectCodexRuntime::derive(base, &project).unwrap();
    std::fs::create_dir_all(&runtime.runtime_home).unwrap();
    std::fs::write(
        runtime.runtime_home.join("config.toml"),
        "[plugins.\"orphan@legacy\"]\nenabled = true\n",
    )
    .unwrap();
    persist_project_codex_runtime(&runtime).unwrap();
    let source_context = AgentContext {
        installation_id: claude.id.clone(),
        project_path: Some(runtime.project_path.clone()),
    };
    let target_context = AgentContext {
        installation_id: runtime.runtime_installation_id.clone(),
        project_path: Some(runtime.project_path.clone()),
    };

    let error = ClaudeToCodexRoute
        .preview_with_options(
            &source_context,
            &target_context,
            &ClaudeToCodexOptions {
                inherit_base_config: false,
                ..ClaudeToCodexOptions::default()
            },
        )
        .unwrap_err();

    match previous_home {
        Some(value) => std::env::set_var("AD_HOME", value),
        None => std::env::remove_var("AD_HOME"),
    }
    match previous_codex_home {
        Some(value) => std::env::set_var("CODEX_HOME", value),
        None => std::env::remove_var("CODEX_HOME"),
    }

    assert_eq!(error.code, ad_lib::agents::AgentErrorCode::ResourceChanged);
    assert!(error.message.contains("ambiguous ownership"));
}

#[test]
#[serial_test::serial(home_env)]
fn inherited_legacy_runtime_accepts_marketplace_revision_drift() {
    let temp = tempfile::tempdir().unwrap();
    let claude_home = temp.path().join(".claude");
    let codex_home = temp.path().join(".codex");
    let project = temp.path().join("project");
    std::fs::create_dir_all(&claude_home).unwrap();
    std::fs::create_dir_all(&codex_home).unwrap();
    std::fs::create_dir_all(&project).unwrap();
    std::fs::create_dir_all(codex_home.join(".tmp/marketplaces/kami/.agents/plugins")).unwrap();
    std::fs::create_dir_all(codex_home.join("plugins/cache/kami/kami/1.0.0/.codex-plugin"))
        .unwrap();
    std::fs::write(
        codex_home.join(".tmp/marketplaces/kami/.agents/plugins/marketplace.json"),
        r#"{"name":"kami"}"#,
    )
    .unwrap();
    std::fs::write(
        codex_home.join("plugins/cache/kami/kami/1.0.0/.codex-plugin/plugin.json"),
        r#"{"name":"kami","version":"1.0.0"}"#,
    )
    .unwrap();
    std::fs::write(
        codex_home.join("config.toml"),
        concat!(
            "cli_auth_credentials_store = \"file\"\n\n",
            "[marketplaces.kami]\n",
            "source_type = \"git\"\n",
            "source = \"https://github.com/tw93/kami.git\"\n",
            "last_revision = \"current-revision\"\n\n",
            "[plugins.\"kami@kami\"]\n",
            "enabled = true\n",
        ),
    )
    .unwrap();
    std::fs::write(codex_home.join("auth.json"), "shared-login").unwrap();
    let previous_home = std::env::var("AD_HOME").ok();
    let previous_codex_home = std::env::var("CODEX_HOME").ok();
    std::env::set_var("AD_HOME", temp.path());
    std::env::remove_var("CODEX_HOME");

    let installations = builtin_registry().discover();
    let claude = installations
        .iter()
        .find(|installation| installation.agent_id.as_str() == "claude-code")
        .unwrap();
    let base = installations
        .iter()
        .find(|installation| installation.agent_id.as_str() == "codex")
        .unwrap();
    let runtime = ProjectCodexRuntime::derive(base, &project).unwrap();
    std::fs::create_dir_all(&runtime.runtime_home).unwrap();
    std::fs::write(
        runtime.runtime_home.join("config.toml"),
        concat!(
            "cli_auth_credentials_store = \"file\"\n\n",
            "[marketplaces.kami]\n",
            "source_type = \"git\"\n",
            "source = \"https://github.com/tw93/kami.git\"\n",
            "last_revision = \"legacy-revision\"\n\n",
            "[plugins.\"kami@kami\"]\n",
            "enabled = true\n",
        ),
    )
    .unwrap();
    persist_project_codex_runtime(&runtime).unwrap();
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
                inherit_base_config: true,
                ..ClaudeToCodexOptions::default()
            },
        )
        .unwrap();

    match previous_home {
        Some(value) => std::env::set_var("AD_HOME", value),
        None => std::env::remove_var("AD_HOME"),
    }
    match previous_codex_home {
        Some(value) => std::env::set_var("CODEX_HOME", value),
        None => std::env::remove_var("CODEX_HOME"),
    }

    assert!(preview
        .plan
        .mutations
        .iter()
        .any(|mutation| mutation.resource.logical_id == "runtime-manifest"));
}

#[test]
#[serial_test::serial(home_env)]
fn isolated_legacy_runtime_requires_explicit_ownership_for_revision_drift() {
    let temp = tempfile::tempdir().unwrap();
    let claude_home = temp.path().join(".claude");
    let codex_home = temp.path().join(".codex");
    let project = temp.path().join("project");
    std::fs::create_dir_all(&claude_home).unwrap();
    std::fs::create_dir_all(&codex_home).unwrap();
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(
        codex_home.join("config.toml"),
        concat!(
            "cli_auth_credentials_store = \"file\"\n\n",
            "[marketplaces.kami]\n",
            "source_type = \"git\"\n",
            "source = \"https://github.com/tw93/kami.git\"\n",
            "last_revision = \"current-revision\"\n\n",
            "[plugins.\"kami@kami\"]\n",
            "enabled = true\n",
        ),
    )
    .unwrap();
    std::fs::write(codex_home.join("auth.json"), "shared-login").unwrap();
    let previous_home = std::env::var("AD_HOME").ok();
    let previous_codex_home = std::env::var("CODEX_HOME").ok();
    std::env::set_var("AD_HOME", temp.path());
    std::env::remove_var("CODEX_HOME");

    let installations = builtin_registry().discover();
    let claude = installations
        .iter()
        .find(|installation| installation.agent_id.as_str() == "claude-code")
        .unwrap();
    let base = installations
        .iter()
        .find(|installation| installation.agent_id.as_str() == "codex")
        .unwrap();
    let runtime = ProjectCodexRuntime::derive(base, &project).unwrap();
    std::fs::create_dir_all(&runtime.runtime_home).unwrap();
    std::fs::write(
        runtime.runtime_home.join("config.toml"),
        concat!(
            "cli_auth_credentials_store = \"file\"\n\n",
            "[marketplaces.kami]\n",
            "source_type = \"git\"\n",
            "source = \"https://github.com/tw93/kami.git\"\n",
            "last_revision = \"legacy-revision\"\n\n",
            "[plugins.\"kami@kami\"]\n",
            "enabled = true\n",
        ),
    )
    .unwrap();
    persist_project_codex_runtime(&runtime).unwrap();
    let source_context = AgentContext {
        installation_id: claude.id.clone(),
        project_path: Some(runtime.project_path.clone()),
    };
    let target_context = AgentContext {
        installation_id: runtime.runtime_installation_id.clone(),
        project_path: Some(runtime.project_path.clone()),
    };

    let error = ClaudeToCodexRoute
        .preview_with_options(
            &source_context,
            &target_context,
            &ClaudeToCodexOptions {
                inherit_base_config: false,
                ..ClaudeToCodexOptions::default()
            },
        )
        .unwrap_err();

    match previous_home {
        Some(value) => std::env::set_var("AD_HOME", value),
        None => std::env::remove_var("AD_HOME"),
    }
    match previous_codex_home {
        Some(value) => std::env::set_var("CODEX_HOME", value),
        None => std::env::remove_var("CODEX_HOME"),
    }

    assert_eq!(error.code, ad_lib::agents::AgentErrorCode::ResourceChanged);
    assert!(error.message.contains("ambiguous ownership"));
}

#[test]
#[serial_test::serial(home_env)]
fn legacy_runtime_drops_disabled_unowned_plugin_metadata() {
    let temp = tempfile::tempdir().unwrap();
    let claude_home = temp.path().join(".claude");
    let codex_home = temp.path().join(".codex");
    let project = temp.path().join("project");
    std::fs::create_dir_all(&claude_home).unwrap();
    std::fs::create_dir_all(&codex_home).unwrap();
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(
        codex_home.join("config.toml"),
        "cli_auth_credentials_store = \"file\"\n",
    )
    .unwrap();
    std::fs::write(codex_home.join("auth.json"), "shared-login").unwrap();
    let previous_home = std::env::var("AD_HOME").ok();
    let previous_codex_home = std::env::var("CODEX_HOME").ok();
    std::env::set_var("AD_HOME", temp.path());
    std::env::remove_var("CODEX_HOME");

    let installations = builtin_registry().discover();
    let claude = installations
        .iter()
        .find(|installation| installation.agent_id.as_str() == "claude-code")
        .unwrap();
    let base = installations
        .iter()
        .find(|installation| installation.agent_id.as_str() == "codex")
        .unwrap();
    let runtime = ProjectCodexRuntime::derive(base, &project).unwrap();
    std::fs::create_dir_all(&runtime.runtime_home).unwrap();
    std::fs::write(
        runtime.runtime_home.join("config.toml"),
        concat!(
            "[marketplaces.legacy]\n",
            "source_type = \"local\"\n",
            "source = \"/tmp/legacy-marketplace\"\n\n",
            "[plugins.\"orphan@legacy\"]\n",
            "enabled = false\n",
        ),
    )
    .unwrap();
    persist_project_codex_runtime(&runtime).unwrap();
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
                inherit_base_config: false,
                ..ClaudeToCodexOptions::default()
            },
        )
        .unwrap();

    let plan_id = preview.plan.id.clone();
    let plans = PlanStore::default();
    plans.insert(preview.plan).unwrap();
    ExecutionEngine.apply(&plan_id, &plans).unwrap();

    let config = std::fs::read_to_string(runtime.runtime_home.join("config.toml"))
        .unwrap()
        .parse::<toml::Value>()
        .unwrap();
    assert!(config.get("plugins").is_none());
    assert!(config.get("marketplaces").is_none());
    let manifest = load_project_codex_runtime_manifest(&runtime)
        .unwrap()
        .unwrap()
        .manifest;
    assert!(manifest.project_overlay.enabled_plugins.is_empty());
    assert!(manifest.project_overlay.marketplaces.is_empty());

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
fn project_route_batches_compatible_plugins_and_activates_config_last() {
    let temp = tempfile::tempdir().unwrap();
    let claude_home = temp.path().join(".claude");
    let codex_home = temp.path().join(".codex");
    let project = temp.path().join("project");
    let marketplace = claude_home.join("plugins/marketplaces/team");
    let alpha = claude_home.join("plugins/cache/team/alpha/1.0.0");
    let beta = claude_home.join("plugins/cache/team/beta/2.0.0");
    let gamma = claude_home.join("plugins/cache/team/gamma/3.0.0");
    std::fs::create_dir_all(&codex_home).unwrap();
    std::fs::create_dir_all(project.join(".claude")).unwrap();
    std::fs::create_dir_all(marketplace.join(".agents/plugins")).unwrap();
    std::fs::create_dir_all(alpha.join(".codex-plugin")).unwrap();
    std::fs::create_dir_all(beta.join(".codex-plugin")).unwrap();
    std::fs::create_dir_all(gamma.join("skills/review")).unwrap();
    std::fs::write(
        codex_home.join("config.toml"),
        concat!(
            "model = \"gpt-5.6\"\n",
            "cli_auth_credentials_store = \"file\"\n\n",
            "[plugins.\"beta@team\"]\n",
            "enabled = false\n",
        ),
    )
    .unwrap();
    std::fs::write(codex_home.join("auth.json"), "shared-login").unwrap();
    write_json(
        &project.join(".claude/settings.json"),
        serde_json::json!({
            "model": "opus",
            "enabledPlugins": {
                "alpha@team": true,
                "beta@team": true,
                "gamma@team": true,
                "disabled@team": false
            }
        }),
    );
    write_json(
        &marketplace.join(".agents/plugins/marketplace.json"),
        serde_json::json!({"name": "team"}),
    );
    write_json(
        &alpha.join(".codex-plugin/plugin.json"),
        serde_json::json!({"name": "alpha", "version": "1.0.0"}),
    );
    write_json(
        &beta.join(".codex-plugin/plugin.json"),
        serde_json::json!({"name": "beta", "version": "2.0.0"}),
    );
    write_json(
        &gamma.join(".claude-plugin/plugin.json"),
        serde_json::json!({
            "name": "gamma",
            "version": "3.0.0",
            "lspServers": {"gamma": {"command": "gamma-lsp"}}
        }),
    );
    std::fs::write(
        gamma.join("skills/review/SKILL.md"),
        "---\nname: review\n---\n",
    )
    .unwrap();
    let canonical_project = std::fs::canonicalize(&project).unwrap();
    write_json(
        &claude_home.join("plugins/installed_plugins.json"),
        serde_json::json!({
            "version": 2,
            "plugins": {
                "alpha@team": [{
                    "scope": "local",
                    "projectPath": canonical_project,
                    "installPath": alpha,
                    "version": "1.0.0",
                    "gitCommitSha": "abc123"
                }],
                "beta@team": [{
                    "scope": "local",
                    "projectPath": canonical_project,
                    "installPath": beta,
                    "version": "2.0.0",
                    "gitCommitSha": "def456"
                }],
                "gamma@team": [{
                    "scope": "local",
                    "projectPath": canonical_project,
                    "installPath": gamma,
                    "version": "3.0.0",
                    "gitCommitSha": "ghi789"
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
    let base = installations
        .iter()
        .find(|installation| {
            installation.agent_id.as_str() == "codex"
                && installation.root_path
                    == std::fs::canonicalize(&codex_home)
                        .unwrap()
                        .to_string_lossy()
        })
        .unwrap();
    let runtime = ProjectCodexRuntime::derive(base, &project).unwrap();
    std::fs::create_dir_all(&runtime.runtime_home).unwrap();
    std::fs::write(
        runtime.runtime_home.join("config.toml"),
        concat!(
            "model = \"gpt-5.6\"\n",
            "cli_auth_credentials_store = \"file\"\n\n",
            "[plugins.\"beta@team\"]\n",
            "enabled = false\n",
        ),
    )
    .unwrap();
    persist_project_codex_runtime(&runtime).unwrap();
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
                target_model: Some("project-api".into()),
                ..ClaudeToCodexOptions::default()
            },
        )
        .unwrap();

    assert_eq!(
        preview
            .artifacts
            .iter()
            .find(|artifact| artifact.id == "plugin:disabled@team")
            .unwrap()
            .disposition,
        ArtifactDisposition::Unchanged
    );
    assert_eq!(
        preview
            .artifacts
            .iter()
            .filter(|artifact| {
                artifact.kind == ad_lib::agents::ResourceKind::Plugins
                    && artifact.disposition == ArtifactDisposition::Mapped
            })
            .count(),
        2
    );
    assert_eq!(
        preview
            .artifacts
            .iter()
            .find(|artifact| artifact.id == "plugin:beta@team")
            .unwrap()
            .disposition,
        ArtifactDisposition::Mapped
    );
    assert_eq!(
        preview
            .artifacts
            .iter()
            .find(|artifact| artifact.id == "plugin:gamma@team")
            .unwrap()
            .disposition,
        ArtifactDisposition::Partial
    );
    assert_eq!(
        preview
            .plan
            .mutations
            .iter()
            .filter(|mutation| mutation.resource.logical_id == "marketplace:team")
            .count(),
        1
    );
    assert!(preview
        .plan
        .mutations
        .last()
        .unwrap()
        .media_type
        .contains("toml"));
    let manifests = preview
        .plan
        .mutations
        .iter()
        .filter(|mutation| mutation.resource.logical_id == "runtime-manifest")
        .collect::<Vec<_>>();
    assert_eq!(manifests.len(), 1);
    let manifest = serde_json::from_value::<ad_lib::agents::ProjectCodexRuntimeManifest>(
        manifests[0].content.clone().unwrap(),
    )
    .unwrap();
    assert!(manifest.applied_inherit_base_config);
    assert_eq!(
        manifest
            .project_settings_keys
            .iter()
            .cloned()
            .collect::<Vec<_>>(),
        vec!["model".to_owned()]
    );
    assert_eq!(
        manifest
            .project_overlay
            .enabled_plugins
            .keys()
            .cloned()
            .collect::<Vec<_>>(),
        vec![
            "alpha@team".to_owned(),
            "beta@team".to_owned(),
            "gamma@team".to_owned(),
        ]
    );

    let plan_id = preview.plan.id.clone();
    let plans = PlanStore::default();
    plans.insert(preview.plan).unwrap();
    ExecutionEngine.apply(&plan_id, &plans).unwrap();

    let config = std::fs::read_to_string(runtime.runtime_home.join("config.toml"))
        .unwrap()
        .parse::<toml::Value>()
        .unwrap();
    assert_eq!(config["model"].as_str(), Some("project-api"));
    assert_eq!(
        config["plugins"]["alpha@team"]["enabled"].as_bool(),
        Some(true)
    );
    assert_eq!(
        config["plugins"]["beta@team"]["enabled"].as_bool(),
        Some(true)
    );
    assert_eq!(
        config["plugins"]["gamma@team"]["enabled"].as_bool(),
        Some(true)
    );
    assert!(runtime
        .runtime_home
        .join("plugins/cache/team/alpha/1.0.0/.codex-plugin/plugin.json")
        .is_file());
    assert!(runtime
        .runtime_home
        .join("plugins/cache/team/gamma/3.0.0/.codex-plugin/plugin.json")
        .is_file());
    assert!(runtime
        .runtime_home
        .join("plugins/cache/team/beta/2.0.0/.codex-plugin/plugin.json")
        .is_file());

    let repeated_preview = ClaudeToCodexRoute
        .preview_with_options(
            &source_context,
            &target_context,
            &ClaudeToCodexOptions::default(),
        )
        .unwrap();
    let repeated_alpha = repeated_preview
        .artifacts
        .iter()
        .find(|artifact| artifact.id == "plugin:alpha@team")
        .unwrap();
    assert_eq!(repeated_alpha.disposition, ArtifactDisposition::Unchanged);
    assert!(repeated_alpha.resolution.is_none());

    std::fs::create_dir_all(alpha.join("skills/new")).unwrap();
    std::fs::write(alpha.join("skills/new/SKILL.md"), "---\nname: new\n---\n").unwrap();
    let installed_alpha = runtime.runtime_home.join("plugins/cache/team/alpha/1.0.0");
    std::fs::create_dir_all(installed_alpha.join("scripts/__pycache__")).unwrap();
    std::fs::write(
        installed_alpha.join("scripts/__pycache__/hook.pyc"),
        b"runtime cache",
    )
    .unwrap();

    let refreshed_preview = ClaudeToCodexRoute
        .preview_with_options(
            &source_context,
            &target_context,
            &ClaudeToCodexOptions::default(),
        )
        .unwrap();
    let refreshed_alpha = refreshed_preview
        .artifacts
        .iter()
        .find(|artifact| artifact.id == "plugin:alpha@team")
        .unwrap();
    assert_eq!(refreshed_alpha.disposition, ArtifactDisposition::Mapped);
    assert!(refreshed_preview
        .plan
        .mutations
        .iter()
        .any(|mutation| mutation.resource.logical_id == "package:team:alpha:1.0.0"));
    let refreshed_plan_id = refreshed_preview.plan.id.clone();
    let refreshed_plans = PlanStore::default();
    refreshed_plans.insert(refreshed_preview.plan).unwrap();
    ExecutionEngine
        .apply(&refreshed_plan_id, &refreshed_plans)
        .unwrap();
    assert!(installed_alpha.join("skills/new/SKILL.md").is_file());
    assert!(!installed_alpha.join("scripts/__pycache__").exists());

    let plugins = registry.adapter("codex").unwrap().plugins().unwrap();
    let alpha_resource = plugins
        .list(&target_context)
        .unwrap()
        .into_iter()
        .find(|snapshot| snapshot.resource.logical_id == "alpha@team")
        .unwrap()
        .resource;
    let toggle = plugins
        .plan_set_enabled(&target_context, &alpha_resource, false)
        .unwrap();
    let toggle_id = toggle.id.clone();
    let toggle_plans = PlanStore::default();
    toggle_plans.insert(toggle).unwrap();
    ExecutionEngine.apply(&toggle_id, &toggle_plans).unwrap();
    let toggled_config = std::fs::read_to_string(runtime.runtime_home.join("config.toml"))
        .unwrap()
        .parse::<toml::Value>()
        .unwrap();
    assert_eq!(toggled_config["model"].as_str(), Some("project-api"));
    assert_eq!(
        toggled_config["plugins"]["alpha@team"]["enabled"].as_bool(),
        Some(false)
    );

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
fn project_route_reuses_user_plugin_from_inherited_base_config() {
    let temp = tempfile::tempdir().unwrap();
    let claude_home = temp.path().join(".claude");
    let codex_home = temp.path().join(".codex");
    let project = temp.path().join("project");
    let claude_marketplace = claude_home.join("plugins/marketplaces/coopArk");
    let claude_package = claude_home.join("plugins/cache/coopArk/skybase-dev-hook/1.1.6");
    let codex_marketplace = codex_home.join("marketplaces/coopArk");
    let codex_package = codex_home.join("plugins/cache/coopArk/skybase-dev-hook/1.1.6");
    std::fs::create_dir_all(project.join(".claude")).unwrap();
    std::fs::create_dir_all(claude_marketplace.join(".claude-plugin")).unwrap();
    std::fs::create_dir_all(claude_package.join(".codex-plugin")).unwrap();
    std::fs::create_dir_all(codex_marketplace.join(".agents/plugins")).unwrap();
    std::fs::create_dir_all(codex_package.join(".codex-plugin")).unwrap();
    write_json(
        &claude_home.join("settings.json"),
        serde_json::json!({
            "enabledPlugins": {"skybase-dev-hook@coopArk": true}
        }),
    );
    write_json(
        &claude_marketplace.join(".claude-plugin/marketplace.json"),
        serde_json::json!({"name": "coopArk"}),
    );
    write_json(
        &claude_package.join(".codex-plugin/plugin.json"),
        serde_json::json!({"name": "skybase-dev-hook", "version": "1.1.6"}),
    );
    write_json(
        &claude_home.join("plugins/installed_plugins.json"),
        serde_json::json!({
            "version": 2,
            "plugins": {
                "skybase-dev-hook@coopArk": [{
                    "scope": "user",
                    "installPath": claude_package,
                    "version": "1.1.6"
                }]
            }
        }),
    );
    write_json(
        &claude_home.join("plugins/known_marketplaces.json"),
        serde_json::json!({
            "coopArk": {
                "source": {"source": "directory", "path": claude_marketplace},
                "installLocation": claude_marketplace
            }
        }),
    );
    write_json(
        &codex_marketplace.join(".agents/plugins/marketplace.json"),
        serde_json::json!({"name": "coopArk"}),
    );
    write_json(
        &codex_package.join(".codex-plugin/plugin.json"),
        serde_json::json!({"name": "skybase-dev-hook", "version": "1.1.6"}),
    );
    std::fs::write(codex_package.join("README.md"), "base package").unwrap();
    std::fs::write(
        codex_home.join("config.toml"),
        format!(
            concat!(
                "cli_auth_credentials_store = \"file\"\n\n",
                "[marketplaces.coopArk]\n",
                "source_type = \"local\"\n",
                "source = \"{}\"\n\n",
                "[plugins.\"skybase-dev-hook@coopArk\"]\n",
                "enabled = true\n",
            ),
            codex_marketplace.display()
        ),
    )
    .unwrap();
    std::fs::write(codex_home.join("auth.json"), "shared-login").unwrap();

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
    let base = installations
        .iter()
        .find(|installation| {
            installation.agent_id.as_str() == "codex"
                && installation.root_path
                    == std::fs::canonicalize(&codex_home)
                        .unwrap()
                        .to_string_lossy()
        })
        .unwrap();
    let runtime = ProjectCodexRuntime::derive(base, &project).unwrap();
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
            &ClaudeToCodexOptions::default(),
        )
        .unwrap();

    assert!(!runtime.runtime_home.exists());
    let artifact = preview
        .artifacts
        .iter()
        .find(|artifact| artifact.id == "plugin:skybase-dev-hook@coopArk")
        .unwrap();
    assert_eq!(artifact.disposition, ArtifactDisposition::Unchanged);
    let manifest = preview
        .plan
        .mutations
        .iter()
        .find(|mutation| mutation.resource.logical_id == "runtime-manifest")
        .and_then(|mutation| mutation.content.clone())
        .map(serde_json::from_value::<ad_lib::agents::ProjectCodexRuntimeManifest>)
        .unwrap()
        .unwrap();
    assert!(manifest.project_overlay.marketplaces.is_empty());
    assert!(manifest.project_overlay.enabled_plugins.is_empty());
    let base_config = std::fs::read(codex_home.join("config.toml")).unwrap();
    let base_package = std::fs::read(codex_package.join("README.md")).unwrap();

    let plan_id = preview.plan.id.clone();
    let plans = PlanStore::default();
    plans.insert(preview.plan).unwrap();
    ExecutionEngine.apply(&plan_id, &plans).unwrap();

    let config = std::fs::read_to_string(runtime.runtime_home.join("config.toml"))
        .unwrap()
        .parse::<toml::Value>()
        .unwrap();
    assert_eq!(
        config["marketplaces"]["coopArk"]["source"].as_str(),
        Some(codex_marketplace.to_string_lossy().as_ref())
    );
    assert_eq!(
        std::fs::read_to_string(
            runtime
                .runtime_home
                .join("plugins/cache/coopArk/skybase-dev-hook/1.1.6/README.md")
        )
        .unwrap(),
        "base package"
    );
    assert_eq!(
        std::fs::read(codex_home.join("config.toml")).unwrap(),
        base_config
    );
    assert_eq!(
        std::fs::read(codex_package.join("README.md")).unwrap(),
        base_package
    );
    write_json(
        &project.join(".claude/settings.local.json"),
        serde_json::json!({
            "enabledPlugins": {"skybase-dev-hook@coopArk": true}
        }),
    );
    let explicit_project_preview = ClaudeToCodexRoute
        .preview_with_options(
            &source_context,
            &target_context,
            &ClaudeToCodexOptions::default(),
        )
        .unwrap();
    let explicit_project_artifact = explicit_project_preview
        .artifacts
        .iter()
        .find(|artifact| artifact.id == "plugin:skybase-dev-hook@coopArk")
        .unwrap();
    assert_eq!(
        explicit_project_artifact.disposition,
        ArtifactDisposition::Conflict
    );
    assert!(explicit_project_preview.plan.mutations.is_empty());
    std::fs::remove_file(project.join(".claude/settings.local.json")).unwrap();

    let inherited_plugin = registry
        .adapter("codex")
        .unwrap()
        .plugins()
        .unwrap()
        .list(&target_context)
        .unwrap()
        .into_iter()
        .find(|snapshot| snapshot.resource.logical_id == "skybase-dev-hook@coopArk")
        .unwrap()
        .resource;
    let disable = registry
        .adapter("codex")
        .unwrap()
        .plugins()
        .unwrap()
        .plan_set_enabled(&target_context, &inherited_plugin, false)
        .unwrap();
    let disable_id = disable.id.clone();
    plans.insert(disable).unwrap();
    ExecutionEngine.apply(&disable_id, &plans).unwrap();

    let disabled_override_preview = ClaudeToCodexRoute
        .preview_with_options(
            &source_context,
            &target_context,
            &ClaudeToCodexOptions::default(),
        )
        .unwrap();
    let disabled_override_artifact = disabled_override_preview
        .artifacts
        .iter()
        .find(|artifact| artifact.id == "plugin:skybase-dev-hook@coopArk")
        .unwrap();
    assert_eq!(
        disabled_override_artifact.disposition,
        ArtifactDisposition::Conflict
    );
    assert!(disabled_override_preview.plan.mutations.is_empty());

    match previous_home {
        Some(value) => std::env::set_var("AD_HOME", value),
        None => std::env::remove_var("AD_HOME"),
    }
    match previous_codex_home {
        Some(value) => std::env::set_var("CODEX_HOME", value),
        None => std::env::remove_var("CODEX_HOME"),
    }
}
