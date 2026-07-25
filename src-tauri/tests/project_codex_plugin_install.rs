use ad_lib::agents::{
    builtin_registry, persist_project_codex_runtime, AgentContext, CollectionInstallRequest,
    ExecutionEngine, OperationStatus, PlanStore, PluginInstallProgress, ProjectCodexRuntime,
};
use std::cell::RefCell;

#[test]
#[serial_test::serial(home_env)]
fn project_plugin_install_applies_official_disk_contract_without_touching_base_home() {
    let temp = tempfile::tempdir().unwrap();
    let base_home = temp.path().join(".codex");
    let project = temp.path().join("project");
    let stage = temp.path().join(".ad/staging/codex-plugin-conversion/demo");
    let marketplace_stage = stage.join("marketplace");
    let package_stage = stage.join("package");
    let second_package_stage = stage.join("second-package");
    let local_marketplace = temp.path().join("bundled-marketplace");
    std::fs::create_dir_all(&base_home).unwrap();
    std::fs::create_dir_all(&project).unwrap();
    std::fs::create_dir_all(marketplace_stage.join(".agents/plugins")).unwrap();
    std::fs::create_dir_all(package_stage.join(".codex-plugin")).unwrap();
    std::fs::create_dir_all(second_package_stage.join(".codex-plugin")).unwrap();
    std::fs::create_dir_all(local_marketplace.join(".agents/plugins")).unwrap();
    std::fs::create_dir_all(base_home.join(".tmp/marketplaces/base-market/.agents/plugins"))
        .unwrap();
    std::fs::create_dir_all(
        base_home.join("plugins/cache/base-market/inherited/3.0.0/.codex-plugin"),
    )
    .unwrap();
    std::fs::create_dir_all(
        base_home.join("plugins/cache/runtime-market/bundled/4.0.0/.codex-plugin"),
    )
    .unwrap();
    let base_config = format!(
        concat!(
            "model = \"gpt-5.6\"\n",
            "cli_auth_credentials_store = \"file\"\n\n",
            "[marketplaces.base-market]\n",
            "source_type = \"git\"\n",
            "source = \"https://github.com/acme/base-plugins.git\"\n\n",
            "[marketplaces.runtime-market]\n",
            "source_type = \"local\"\n",
            "source = \"{}\"\n\n",
            "[plugins.\"inherited@base-market\"]\n",
            "enabled = true\n\n",
            "[plugins.\"bundled@runtime-market\"]\n",
            "enabled = true\n",
        ),
        local_marketplace.display()
    );
    std::fs::write(base_home.join("config.toml"), &base_config).unwrap();
    std::fs::write(base_home.join("auth.json"), "shared-user-login").unwrap();
    std::fs::write(
        marketplace_stage.join(".agents/plugins/marketplace.json"),
        r#"{"name":"team"}"#,
    )
    .unwrap();
    std::fs::write(
        package_stage.join(".codex-plugin/plugin.json"),
        r#"{"name":"demo","version":"1.2.3"}"#,
    )
    .unwrap();
    std::fs::write(package_stage.join("README.md"), "demo").unwrap();
    std::fs::write(
        second_package_stage.join(".codex-plugin/plugin.json"),
        r#"{"name":"second","version":"2.0.0"}"#,
    )
    .unwrap();
    std::fs::write(
        base_home.join(".tmp/marketplaces/base-market/.agents/plugins/marketplace.json"),
        r#"{"name":"base-market"}"#,
    )
    .unwrap();
    std::fs::write(
        base_home.join("plugins/cache/base-market/inherited/3.0.0/.codex-plugin/plugin.json"),
        r#"{"name":"inherited","version":"3.0.0"}"#,
    )
    .unwrap();
    std::fs::write(
        local_marketplace.join(".agents/plugins/marketplace.json"),
        r#"{"name":"runtime-market"}"#,
    )
    .unwrap();
    std::fs::write(
        base_home.join("plugins/cache/runtime-market/bundled/4.0.0/.codex-plugin/plugin.json"),
        r#"{"name":"bundled","version":"4.0.0"}"#,
    )
    .unwrap();

    let previous_home = std::env::var("AD_HOME").ok();
    let previous_codex_home = std::env::var("CODEX_HOME").ok();
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
    let port = registry.adapter("codex").unwrap().plugins().unwrap();
    let progress = RefCell::new(Vec::new());
    let plan = port
        .plan_install_with_progress(
            &context,
            CollectionInstallRequest {
                logical_id: "demo@team".into(),
                source: serde_json::json!({
                    "marketplace": {
                        "name": "team",
                        "sourceType": "git",
                        "source": "https://github.com/acme/plugins.git",
                        "refName": "main",
                        "lastRevision": "abc123",
                        "stagePath": marketplace_stage,
                    },
                    "package": {
                        "name": "demo",
                        "version": "1.2.3",
                        "stagePath": package_stage,
                    }
                }),
            },
            &|event| progress.borrow_mut().push(event),
        )
        .unwrap();

    assert_eq!(
        progress.into_inner(),
        vec![
            PluginInstallProgress {
                logical_id: "bundled@runtime-market".into(),
                current: 1,
                total: 2,
            },
            PluginInstallProgress {
                logical_id: "inherited@base-market".into(),
                current: 2,
                total: 2,
            },
        ]
    );

    assert_eq!(
        plan.mutations
            .iter()
            .map(|mutation| mutation.resource.logical_id.as_str())
            .collect::<Vec<_>>(),
        vec![
            "runtime-auth",
            "marketplace:base-market",
            "marketplace:runtime-market",
            "marketplace:team",
            "package:base-market:inherited:3.0.0",
            "package:runtime-market:bundled:4.0.0",
            "package:team:demo:1.2.3",
            "runtime-manifest",
            "demo@team",
        ]
    );
    assert_eq!(
        plan.mutations
            .iter()
            .find(|mutation| mutation.resource.logical_id == "marketplace:runtime-market")
            .unwrap()
            .content
            .as_ref()
            .unwrap()["path"],
        std::fs::canonicalize(&local_marketplace)
            .unwrap()
            .to_string_lossy()
            .as_ref()
    );
    assert_eq!(
        plan.mutations
            .iter()
            .find(|mutation| {
                mutation.resource.logical_id == "package:runtime-market:bundled:4.0.0"
            })
            .unwrap()
            .content
            .as_ref()
            .unwrap()["path"],
        std::fs::canonicalize(base_home.join("plugins/cache/runtime-market/bundled/4.0.0"))
            .unwrap()
            .to_string_lossy()
            .as_ref()
    );
    assert_eq!(
        std::fs::read_to_string(base_home.join("config.toml")).unwrap(),
        base_config
    );

    let plan_id = plan.id.clone();
    let plans = PlanStore::default();
    plans.insert(plan).unwrap();
    let receipt = ExecutionEngine.apply(&plan_id, &plans).unwrap();

    assert_eq!(receipt.status, OperationStatus::Complete);
    assert_eq!(
        std::fs::read_link(runtime.runtime_home.join("auth.json")).unwrap(),
        std::fs::canonicalize(&base_home).unwrap().join("auth.json")
    );
    assert!(runtime
        .runtime_home
        .join(".tmp/marketplaces/team/.agents/plugins/marketplace.json")
        .is_file());
    assert!(runtime
        .runtime_home
        .join("plugins/cache/team/demo/1.2.3/.codex-plugin/plugin.json")
        .is_file());
    assert!(runtime
        .runtime_home
        .join("plugins/cache/base-market/inherited/3.0.0/.codex-plugin/plugin.json")
        .is_file());
    assert!(runtime
        .runtime_home
        .join("plugins/cache/runtime-market/bundled/4.0.0/.codex-plugin/plugin.json")
        .is_file());
    let generated = std::fs::read_to_string(runtime.runtime_home.join("config.toml")).unwrap();
    let generated = generated.parse::<toml::Value>().unwrap();
    assert_eq!(generated["model"].as_str(), Some("gpt-5.6"));
    assert_eq!(
        generated["plugins"]["demo@team"]["enabled"].as_bool(),
        Some(true)
    );
    assert_eq!(
        generated["plugins"]["inherited@base-market"]["enabled"].as_bool(),
        Some(true)
    );
    assert_eq!(
        generated["plugins"]["bundled@runtime-market"]["enabled"].as_bool(),
        Some(true)
    );
    assert_eq!(
        generated["marketplaces"]["team"]["last_revision"].as_str(),
        Some("abc123")
    );
    assert_eq!(
        std::fs::read_to_string(base_home.join("config.toml")).unwrap(),
        base_config
    );
    assert!(base_home
        .join("plugins/cache/base-market/inherited/3.0.0/.codex-plugin/plugin.json")
        .is_file());

    let repeated = port
        .plan_install(
            &context,
            CollectionInstallRequest {
                logical_id: "demo@team".into(),
                source: serde_json::json!({
                    "marketplace": {
                        "name": "team",
                        "sourceType": "git",
                        "source": "https://github.com/acme/plugins.git",
                        "refName": "main",
                        "lastRevision": "abc123",
                        "stagePath": marketplace_stage,
                    },
                    "package": {
                        "name": "demo",
                        "version": "1.2.3",
                        "stagePath": package_stage,
                    }
                }),
            },
        )
        .unwrap();
    assert!(repeated.mutations.is_empty());
    let demo_resource = port
        .list(&context)
        .unwrap()
        .into_iter()
        .find(|snapshot| snapshot.resource.logical_id == "demo@team")
        .unwrap()
        .resource;
    let disable = port
        .plan_set_enabled(&context, &demo_resource, false)
        .unwrap();
    let manifest = disable
        .mutations
        .iter()
        .find(|mutation| mutation.resource.logical_id == "runtime-manifest")
        .unwrap();
    let manifest = serde_json::from_value::<ad_lib::agents::ProjectCodexRuntimeManifest>(
        manifest.content.clone().unwrap(),
    )
    .unwrap();
    assert_eq!(
        manifest.project_overlay.enabled_plugins.get("demo@team"),
        Some(&false)
    );
    let config = disable
        .mutations
        .iter()
        .find(|mutation| mutation.resource.logical_id == "demo@team")
        .and_then(|mutation| mutation.content.as_ref())
        .and_then(serde_json::Value::as_str)
        .unwrap()
        .parse::<toml::Value>()
        .unwrap();
    assert_eq!(
        config["plugins"]["demo@team"]["enabled"].as_bool(),
        Some(false)
    );

    let second_plan = port
        .plan_install(
            &context,
            CollectionInstallRequest {
                logical_id: "second@team".into(),
                source: serde_json::json!({
                    "marketplace": {
                        "name": "team",
                        "sourceType": "git",
                        "source": "https://github.com/acme/plugins.git",
                        "refName": "main",
                        "lastRevision": "abc123",
                        "stagePath": marketplace_stage,
                    },
                    "package": {
                        "name": "second",
                        "version": "2.0.0",
                        "stagePath": second_package_stage,
                    }
                }),
            },
        )
        .unwrap();
    let second_plan_id = second_plan.id.clone();
    plans.insert(second_plan).unwrap();
    let second_receipt = ExecutionEngine.apply(&second_plan_id, &plans).unwrap();
    let generated = std::fs::read_to_string(runtime.runtime_home.join("config.toml")).unwrap();
    let generated = generated.parse::<toml::Value>().unwrap();
    assert_eq!(
        generated["plugins"]["demo@team"]["enabled"].as_bool(),
        Some(true)
    );
    assert_eq!(
        generated["plugins"]["second@team"]["enabled"].as_bool(),
        Some(true)
    );
    assert!(runtime
        .runtime_home
        .join("plugins/cache/team/second/2.0.0/.codex-plugin/plugin.json")
        .is_file());

    let second_rollback = ExecutionEngine.rollback(&second_receipt.id).unwrap();
    assert_eq!(second_rollback.status, OperationStatus::Complete);

    let rollback = ExecutionEngine.rollback(&receipt.id).unwrap();
    assert_eq!(rollback.status, OperationStatus::Complete);
    assert!(!runtime.runtime_home.join("auth.json").exists());
    assert!(!runtime.runtime_home.join(".tmp/marketplaces/team").exists());
    assert!(!runtime
        .runtime_home
        .join("plugins/cache/team/demo/1.2.3")
        .exists());
    assert!(!runtime
        .runtime_home
        .join("plugins/cache/base-market/inherited/3.0.0")
        .exists());
    assert!(!runtime
        .runtime_home
        .join("plugins/cache/runtime-market/bundled/4.0.0")
        .exists());
    assert!(!runtime.runtime_home.join("config.toml").exists());

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
fn project_plugin_install_rejects_sources_outside_conversion_staging() {
    let temp = tempfile::tempdir().unwrap();
    let base_home = temp.path().join(".codex");
    let project = temp.path().join("project");
    let marketplace_stage = temp.path().join(".ad/not-staging/marketplace");
    let package_stage = temp.path().join(".ad/not-staging/package");
    std::fs::create_dir_all(&base_home).unwrap();
    std::fs::create_dir_all(&project).unwrap();
    std::fs::create_dir_all(&marketplace_stage).unwrap();
    std::fs::create_dir_all(&package_stage).unwrap();
    std::fs::write(
        base_home.join("config.toml"),
        "cli_auth_credentials_store = \"file\"\n",
    )
    .unwrap();
    std::fs::write(base_home.join("auth.json"), "shared-user-login").unwrap();

    let previous_home = std::env::var("AD_HOME").ok();
    let previous_codex_home = std::env::var("CODEX_HOME").ok();
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
        installation_id: runtime.runtime_installation_id,
        project_path: Some(runtime.project_path),
    };

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
                    "marketplace": {
                        "name": "team",
                        "sourceType": "git",
                        "source": "https://github.com/acme/plugins.git",
                        "stagePath": marketplace_stage,
                    },
                    "package": {
                        "name": "demo",
                        "version": "1.2.3",
                        "stagePath": package_stage,
                    }
                }),
            },
        )
        .unwrap_err();

    assert_eq!(error.code, ad_lib::agents::AgentErrorCode::PermissionDenied);
    assert!(error.message.contains("conversion staging"));

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
fn project_plugin_install_blocks_keychain_only_base_auth() {
    let temp = tempfile::tempdir().unwrap();
    let base_home = temp.path().join(".codex");
    let project = temp.path().join("project");
    let marketplace_stage = temp
        .path()
        .join(".ad/staging/codex-plugin-conversion/keychain/marketplace");
    let package_stage = temp
        .path()
        .join(".ad/staging/codex-plugin-conversion/keychain/package");
    std::fs::create_dir_all(&base_home).unwrap();
    std::fs::create_dir_all(&project).unwrap();
    std::fs::create_dir_all(&marketplace_stage).unwrap();
    std::fs::create_dir_all(&package_stage).unwrap();
    std::fs::write(
        base_home.join("config.toml"),
        "cli_auth_credentials_store = \"keyring\"\n",
    )
    .unwrap();
    let previous_home = std::env::var("AD_HOME").ok();
    let previous_codex_home = std::env::var("CODEX_HOME").ok();
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
        installation_id: runtime.runtime_installation_id,
        project_path: Some(runtime.project_path),
    };

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
                    "marketplace": {
                        "name": "team",
                        "sourceType": "git",
                        "source": "https://github.com/acme/plugins.git",
                        "stagePath": marketplace_stage,
                    },
                    "package": {
                        "name": "demo",
                        "version": "1.2.3",
                        "stagePath": package_stage,
                    }
                }),
            },
        )
        .unwrap_err();

    assert_eq!(error.code, ad_lib::agents::AgentErrorCode::Unsupported);
    assert!(error.message.contains("Keychain"));

    match previous_home {
        Some(value) => std::env::set_var("AD_HOME", value),
        None => std::env::remove_var("AD_HOME"),
    }
    match previous_codex_home {
        Some(value) => std::env::set_var("CODEX_HOME", value),
        None => std::env::remove_var("CODEX_HOME"),
    }
}
