use std::collections::BTreeMap;

use ad_lib::agents::{
    builtin_registry, load_project_codex_runtime_manifest, persist_project_codex_runtime,
    render_project_codex_runtime_manifest, MarketplaceOverlay, ProjectCodexRuntime,
    ProjectCodexRuntimeDescriptor, ProjectCodexRuntimeManifest, ProjectPluginOverlay,
    SharedAuthBinding, PROJECT_CODEX_RUNTIME_MANIFEST_MAX_BYTES,
    PROJECT_CODEX_RUNTIME_MANIFEST_MAX_COLLECTION_ENTRIES,
    PROJECT_CODEX_RUNTIME_MANIFEST_SCHEMA_VERSION,
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

fn isolated_manifest() -> ProjectCodexRuntimeManifest {
    ProjectCodexRuntimeManifest {
        schema_version: PROJECT_CODEX_RUNTIME_MANIFEST_SCHEMA_VERSION,
        applied_inherit_base_config: false,
        applied_profile_id: None,
        project_overlay: ProjectPluginOverlay {
            marketplaces: BTreeMap::from([(
                "team".into(),
                MarketplaceOverlay {
                    source_type: "git".into(),
                    source: "https://github.com/acme/plugins.git".into(),
                    ref_name: Some("main".into()),
                    last_revision: Some("abc123".into()),
                },
            )]),
            enabled_plugins: BTreeMap::from([("demo@team".into(), true)]),
        },
        project_settings_keys: Default::default(),
    }
}

fn write_manifest(runtime: &ProjectCodexRuntime, manifest: &ProjectCodexRuntimeManifest) {
    let path = runtime.manifest_path();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(
        path,
        render_project_codex_runtime_manifest(manifest).unwrap(),
    )
    .unwrap();
}

#[test]
#[serial_test::serial(home_env)]
fn runtime_descriptor_derivation_has_no_filesystem_side_effects() {
    let temp = tempfile::tempdir().unwrap();
    let base_home = temp.path().join(".codex");
    let project = temp.path().join("project");
    std::fs::create_dir_all(&base_home).unwrap();
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
    let descriptor = ProjectCodexRuntimeDescriptor::derive(&base, &project).unwrap();

    assert!(!descriptor.runtime_home.exists());
    assert!(!temp
        .path()
        .join(".ad/state/codex-project-runtimes")
        .exists());
    assert_eq!(
        descriptor.manifest_path(),
        descriptor.runtime_home.join(".ad/runtime-manifest.json")
    );

    restore_env(previous_home, previous_codex_home);
}

#[test]
#[serial_test::serial(home_env)]
fn derived_runtime_identity_is_stable_for_the_project_and_base_installation() {
    let temp = tempfile::tempdir().unwrap();
    let base_home = temp.path().join(".codex");
    let project = temp.path().join("project");
    std::fs::create_dir_all(&base_home).unwrap();
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
    let canonical_project = std::fs::canonicalize(&project).unwrap();
    let first = ProjectCodexRuntime::derive(&base, &canonical_project).unwrap();
    let second = ProjectCodexRuntime::derive(&base, &canonical_project).unwrap();

    restore_env(previous_home, previous_codex_home);

    assert_eq!(first, second);
    assert_eq!(first.base_installation_id, base.id);
    assert_eq!(first.project_path, canonical_project.to_string_lossy());
    assert!(first.runtime_home.starts_with(
        std::fs::canonicalize(temp.path())
            .unwrap()
            .join(".ad/codex-homes")
    ));
    assert_ne!(first.runtime_installation_id, first.base_installation_id);
}

#[test]
#[serial_test::serial(home_env)]
fn registered_runtime_is_discovered_and_launches_with_its_codex_home() {
    let temp = tempfile::tempdir().unwrap();
    let base_home = temp.path().join(".codex");
    let project = temp.path().join("project");
    std::fs::create_dir_all(&base_home).unwrap();
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
    let canonical_project = std::fs::canonicalize(&project).unwrap();
    let mut runtime = ProjectCodexRuntime::derive(&base, &canonical_project).unwrap();
    runtime.profile_id = Some("project-api".into());
    std::fs::create_dir_all(&runtime.runtime_home).unwrap();
    persist_project_codex_runtime(&runtime).unwrap();

    let registry = builtin_registry();
    let installation = registry
        .discover()
        .into_iter()
        .find(|installation| installation.id == runtime.runtime_installation_id)
        .unwrap();
    assert_eq!(
        installation.project_path.as_deref(),
        Some(runtime.project_path.as_str())
    );
    assert_eq!(
        installation.base_installation_id.as_ref(),
        Some(&runtime.base_installation_id)
    );
    let context = ad_lib::agents::AgentContext {
        installation_id: installation.id,
        project_path: Some(runtime.project_path.clone()),
    };
    let recipe = registry
        .adapter("codex")
        .unwrap()
        .launcher()
        .unwrap()
        .recipe(&context)
        .unwrap();

    restore_env(previous_home, previous_codex_home);

    assert_eq!(
        recipe.env,
        BTreeMap::from([(
            "CODEX_HOME".to_string(),
            runtime.runtime_home.to_string_lossy().into_owned(),
        )])
    );
    assert_eq!(recipe.cwd, runtime.project_path);
    assert_eq!(recipe.args, vec!["--profile", "project-api"]);
}

#[test]
#[serial_test::serial(home_env)]
fn derived_runtime_rejects_a_different_project_context() {
    let temp = tempfile::tempdir().unwrap();
    let base_home = temp.path().join(".codex");
    let project = temp.path().join("project");
    let other_project = temp.path().join("other-project");
    std::fs::create_dir_all(&base_home).unwrap();
    std::fs::create_dir_all(&project).unwrap();
    std::fs::create_dir_all(&other_project).unwrap();
    let previous_home = std::env::var("AD_HOME").ok();
    let previous_codex_home = std::env::var("CODEX_HOME").ok();
    std::env::set_var("AD_HOME", temp.path());
    std::env::remove_var("CODEX_HOME");

    let registry = builtin_registry();
    let base = registry
        .discover()
        .into_iter()
        .find(|installation| installation.agent_id.as_str() == "codex")
        .unwrap();
    let runtime =
        ProjectCodexRuntime::derive(&base, &std::fs::canonicalize(&project).unwrap()).unwrap();
    std::fs::create_dir_all(&runtime.runtime_home).unwrap();
    persist_project_codex_runtime(&runtime).unwrap();
    let context = ad_lib::agents::AgentContext {
        installation_id: runtime.runtime_installation_id.clone(),
        project_path: Some(
            std::fs::canonicalize(other_project)
                .unwrap()
                .to_string_lossy()
                .into_owned(),
        ),
    };
    let error = builtin_registry()
        .adapter("codex")
        .unwrap()
        .launcher()
        .unwrap()
        .recipe(&context)
        .unwrap_err();

    restore_env(previous_home, previous_codex_home);

    assert_eq!(error.code, ad_lib::agents::AgentErrorCode::InvalidPlan);
    assert!(error.message.contains("different project"));
}

#[test]
#[serial_test::serial(home_env)]
fn shared_auth_binding_distinguishes_file_and_keychain_storage() {
    let temp = tempfile::tempdir().unwrap();
    let base_home = temp.path().join(".codex");
    let runtime_home = temp.path().join("runtime");
    std::fs::create_dir_all(&base_home).unwrap();
    std::fs::write(base_home.join("auth.json"), "opaque-credential-bytes").unwrap();

    let file_binding = SharedAuthBinding::detect(&base_home, &runtime_home, None).unwrap();
    assert_eq!(
        file_binding,
        SharedAuthBinding::FileSymlink {
            source: base_home.join("auth.json"),
            target: runtime_home.join("auth.json"),
        }
    );

    std::fs::remove_file(base_home.join("auth.json")).unwrap();
    let keychain = SharedAuthBinding::detect(&base_home, &runtime_home, Some("keyring")).unwrap();
    assert_eq!(keychain, SharedAuthBinding::KeychainRequiresSharedHome);

    let missing = SharedAuthBinding::detect(&base_home, &runtime_home, Some("file")).unwrap();
    assert_eq!(missing, SharedAuthBinding::MissingBaseLogin);
}

#[test]
#[serial_test::serial(home_env)]
fn runtime_status_reports_freshness_plugins_and_shared_auth_without_reading_credentials() {
    let temp = tempfile::tempdir().unwrap();
    let base_home = temp.path().join(".codex");
    let project = temp.path().join("project");
    std::fs::create_dir_all(&base_home).unwrap();
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(
        base_home.join("config.toml"),
        "cli_auth_credentials_store = \"file\"\n",
    )
    .unwrap();
    std::fs::write(base_home.join("auth.json"), "opaque-secret").unwrap();
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
        "[plugins.\"demo@team\"]\nenabled = true\n",
    )
    .unwrap();
    std::os::unix::fs::symlink(
        std::fs::canonicalize(&base_home).unwrap().join("auth.json"),
        runtime.runtime_home.join("auth.json"),
    )
    .unwrap();
    persist_project_codex_runtime(&runtime).unwrap();
    let runtime =
        ad_lib::agents::refresh_project_codex_runtime_digests(&runtime.runtime_installation_id)
            .unwrap()
            .unwrap();

    let status = ad_lib::agents::inspect_project_codex_runtime_status(&runtime, true).unwrap();
    let policy_mismatch =
        ad_lib::agents::inspect_project_codex_runtime_status(&runtime, false).unwrap();

    restore_env(previous_home, previous_codex_home);
    assert!(status.prepared);
    assert!(status.fresh);
    assert!(status.desired_inherit_base_config);
    assert!(status.applied_inherit_base_config);
    assert!(!status.needs_refresh);
    assert!(!policy_mismatch.fresh);
    assert!(policy_mismatch.needs_refresh);
    assert_eq!(status.plugin_count, 1);
    assert_eq!(
        status.auth_mode,
        ad_lib::agents::ProjectCodexAuthMode::SharedFile
    );
}

#[test]
#[serial_test::serial(home_env)]
fn manifest_backed_isolated_runtime_ignores_base_config_drift() {
    let temp = tempfile::tempdir().unwrap();
    let base_home = temp.path().join(".codex");
    let project = temp.path().join("project");
    std::fs::create_dir_all(&base_home).unwrap();
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(
        base_home.join("config.toml"),
        "model = \"gpt-5.6\"\ncli_auth_credentials_store = \"file\"\n",
    )
    .unwrap();
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
        "cli_auth_credentials_store = \"file\"\n",
    )
    .unwrap();
    write_manifest(&runtime, &isolated_manifest());
    persist_project_codex_runtime(&runtime).unwrap();

    let refreshed =
        ad_lib::agents::refresh_project_codex_runtime_digests(&runtime.runtime_installation_id)
            .unwrap()
            .unwrap();
    assert!(!refreshed.applied_inherit_base_config);
    assert!(refreshed.manifest_digest.is_some());
    assert!(ad_lib::agents::project_codex_runtime_is_fresh(&refreshed).unwrap());

    std::fs::write(
        base_home.join("config.toml"),
        "model = \"gpt-5.7\"\ncli_auth_credentials_store = \"file\"\n",
    )
    .unwrap();
    assert!(ad_lib::agents::project_codex_runtime_is_fresh(&refreshed).unwrap());

    std::fs::write(
        runtime.runtime_home.join("config.toml"),
        "cli_auth_credentials_store = \"file\"\nfeatures = { changed = true }\n",
    )
    .unwrap();
    assert!(!ad_lib::agents::project_codex_runtime_is_fresh(&refreshed).unwrap());

    restore_env(previous_home, previous_codex_home);
}

#[test]
#[serial_test::serial(home_env)]
fn legacy_runtime_without_manifest_still_tracks_base_config() {
    let temp = tempfile::tempdir().unwrap();
    let base_home = temp.path().join(".codex");
    let project = temp.path().join("project");
    std::fs::create_dir_all(&base_home).unwrap();
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(base_home.join("config.toml"), "model = \"gpt-5.6\"\n").unwrap();
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
        "model = \"gpt-5.6\"\n",
    )
    .unwrap();
    persist_project_codex_runtime(&runtime).unwrap();
    let refreshed =
        ad_lib::agents::refresh_project_codex_runtime_digests(&runtime.runtime_installation_id)
            .unwrap()
            .unwrap();

    assert!(refreshed.applied_inherit_base_config);
    assert!(refreshed.manifest_digest.is_none());
    assert!(ad_lib::agents::project_codex_runtime_is_fresh(&refreshed).unwrap());
    std::fs::write(base_home.join("config.toml"), "model = \"gpt-5.7\"\n").unwrap();
    assert!(!ad_lib::agents::project_codex_runtime_is_fresh(&refreshed).unwrap());

    restore_env(previous_home, previous_codex_home);
}

#[test]
#[serial_test::serial(home_env)]
fn runtime_manifest_rejects_unknown_versions_fields_and_credential_urls() {
    let temp = tempfile::tempdir().unwrap();
    let base_home = temp.path().join(".codex");
    let project = temp.path().join("project");
    std::fs::create_dir_all(&base_home).unwrap();
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
    std::fs::create_dir_all(runtime.manifest_path().parent().unwrap()).unwrap();

    let unknown_version = serde_json::json!({
        "schemaVersion": PROJECT_CODEX_RUNTIME_MANIFEST_SCHEMA_VERSION + 1,
        "appliedInheritBaseConfig": false,
        "appliedProfileId": null,
        "projectOverlay": {"marketplaces": {}, "enabledPlugins": {}},
    });
    std::fs::write(
        runtime.manifest_path(),
        serde_json::to_vec(&unknown_version).unwrap(),
    )
    .unwrap();
    assert!(load_project_codex_runtime_manifest(&runtime).is_err());

    let unknown_field = serde_json::json!({
        "schemaVersion": PROJECT_CODEX_RUNTIME_MANIFEST_SCHEMA_VERSION,
        "appliedInheritBaseConfig": false,
        "appliedProfileId": null,
        "projectOverlay": {"marketplaces": {}, "enabledPlugins": {}},
        "authToken": "must-not-be-accepted",
    });
    std::fs::write(
        runtime.manifest_path(),
        serde_json::to_vec(&unknown_field).unwrap(),
    )
    .unwrap();
    assert!(load_project_codex_runtime_manifest(&runtime).is_err());

    let mut credential_manifest = isolated_manifest();
    credential_manifest
        .project_overlay
        .marketplaces
        .get_mut("team")
        .unwrap()
        .source = "https://secret-token@github.com/acme/plugins.git".into();
    assert!(render_project_codex_runtime_manifest(&credential_manifest).is_err());

    restore_env(previous_home, previous_codex_home);
}

#[test]
fn runtime_manifest_enforces_byte_collection_and_string_limits() {
    let mut too_many = isolated_manifest();
    too_many.project_overlay.enabled_plugins = (0
        ..=PROJECT_CODEX_RUNTIME_MANIFEST_MAX_COLLECTION_ENTRIES)
        .map(|index| (format!("demo-{index}@team"), true))
        .collect();
    assert!(render_project_codex_runtime_manifest(&too_many).is_err());

    let mut long_string = isolated_manifest();
    long_string.applied_profile_id = Some("x".repeat(101));
    assert!(render_project_codex_runtime_manifest(&long_string).is_err());

    let temp = tempfile::tempdir().unwrap();
    let runtime_home = temp.path().join("runtime");
    std::fs::create_dir_all(runtime_home.join(".ad")).unwrap();
    let runtime = ProjectCodexRuntime {
        project_id: "placeholder".into(),
        project_path: temp.path().to_string_lossy().into_owned(),
        base_installation_id: "codex:base".into(),
        runtime_installation_id: "codex:runtime".into(),
        runtime_home,
        base_config_digest: None,
        generated_config_digest: None,
        profile_id: None,
        applied_inherit_base_config: true,
        manifest_digest: None,
    };
    std::fs::write(
        runtime.manifest_path(),
        vec![b' '; PROJECT_CODEX_RUNTIME_MANIFEST_MAX_BYTES + 1],
    )
    .unwrap();
    assert!(load_project_codex_runtime_manifest(&runtime).is_err());
}

#[test]
#[serial_test::serial(home_env)]
fn missing_or_corrupt_applied_manifest_fails_closed() {
    let temp = tempfile::tempdir().unwrap();
    let base_home = temp.path().join(".codex");
    let project = temp.path().join("project");
    std::fs::create_dir_all(&base_home).unwrap();
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(base_home.join("config.toml"), "model = \"gpt-5.6\"\n").unwrap();
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
        "model = \"gpt-5.6\"\n",
    )
    .unwrap();
    write_manifest(&runtime, &isolated_manifest());
    persist_project_codex_runtime(&runtime).unwrap();
    let refreshed =
        ad_lib::agents::refresh_project_codex_runtime_digests(&runtime.runtime_installation_id)
            .unwrap()
            .unwrap();

    std::fs::remove_file(runtime.manifest_path()).unwrap();
    assert!(!ad_lib::agents::project_codex_runtime_is_fresh(&refreshed).unwrap());

    std::fs::write(runtime.manifest_path(), b"{not-json").unwrap();
    assert!(ad_lib::agents::project_codex_runtime_is_fresh(&refreshed).is_err());

    restore_env(previous_home, previous_codex_home);
}
