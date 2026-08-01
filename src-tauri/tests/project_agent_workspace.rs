use ad_lib::agents::{
    builtin_registry, inspect_project_workspace_inventory, persist_project_codex_runtime,
    render_project_codex_runtime_manifest, resolve_project_agent_workspace, AgentContext,
    AgentErrorCode, ProjectCodexRuntime, ProjectCodexRuntimeManifest, ProjectPluginOverlay,
    SettingsEdit, PROJECT_CODEX_RUNTIME_MANIFEST_SCHEMA_VERSION,
};
use std::collections::{BTreeMap, BTreeSet};

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

#[test]
#[serial_test::serial(home_env)]
fn claude_inventory_reports_layer_winners_and_masks_sensitive_values() {
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("project");
    std::fs::create_dir_all(temp.path().join(".claude/skills/user-skill")).unwrap();
    std::fs::create_dir_all(project.join(".claude/skills/project-skill")).unwrap();
    std::fs::write(
        temp.path().join(".claude/skills/user-skill/SKILL.md"),
        "# User\n",
    )
    .unwrap();
    std::fs::write(
        project.join(".claude/skills/project-skill/SKILL.md"),
        "# Project\n",
    )
    .unwrap();
    std::fs::write(
        temp.path().join(".claude/settings.json"),
        r#"{"model":"user","apiToken":"secret-value","enabledPlugins":{"demo":true}}"#,
    )
    .unwrap();
    std::fs::write(
        project.join(".claude/settings.json"),
        r#"{"model":"shared","futureSetting":{"enabled":true},"enabledPlugins":{"demo":true}}"#,
    )
    .unwrap();
    std::fs::write(
        project.join(".claude/settings.local.json"),
        r#"{"model":"local","projectToken":"project-secret","enabledPlugins":{"demo":false}}"#,
    )
    .unwrap();
    let previous_home = std::env::var("AD_HOME").ok();
    let previous_codex_home = std::env::var("CODEX_HOME").ok();
    std::env::set_var("AD_HOME", temp.path());
    std::env::remove_var("CODEX_HOME");

    let installation = builtin_registry()
        .discover()
        .into_iter()
        .find(|installation| installation.agent_id.as_str() == "claude-code")
        .unwrap();
    let inventory = inspect_project_workspace_inventory(&installation.id, &project).unwrap();

    let editable = inventory
        .settings
        .editable_targets
        .iter()
        .find(|target| target.resource.logical_id == "project-local")
        .unwrap();
    let layer = inventory
        .settings
        .layers
        .iter()
        .find(|layer| layer.declaration.key == editable.declaration_key)
        .unwrap();
    let mut proposed = layer.content.clone();
    proposed["model"] = serde_json::Value::String("next".into());
    let plan = builtin_registry()
        .adapter("claude-code")
        .unwrap()
        .settings()
        .unwrap()
        .plan_edit(
            &AgentContext {
                installation_id: inventory.workspace.effective_installation_id.clone(),
                project_path: Some(inventory.workspace.canonical_project_path.clone()),
            },
            SettingsEdit {
                resource: editable.resource.clone(),
                media_type: editable.media_type.clone(),
                content: proposed,
            },
        )
        .unwrap();
    std::fs::write(
        temp.path().join(".claude/settings.json"),
        r#"{"model":"user","apiToken":"rotated-secret","enabledPlugins":{"demo":true}}"#,
    )
    .unwrap();
    let rotated_inventory =
        inspect_project_workspace_inventory(&installation.id, &project).unwrap();

    restore_env(previous_home, previous_codex_home);

    assert_eq!(inventory.workspace.agent_id.as_str(), "claude-code");
    assert_eq!(
        inventory.settings.coverage.status,
        ad_lib::agents::CoverageStatus::Partial
    );
    assert_eq!(inventory.settings.effective_content["model"], "local");
    assert_eq!(inventory.settings.effective_content["apiToken"], "••••••••");
    assert!(!serde_json::to_string(&inventory)
        .unwrap()
        .contains("secret-value"));
    assert!(!serde_json::to_string(&inventory)
        .unwrap()
        .contains("project-secret"));
    assert_ne!(inventory.revision, rotated_inventory.revision);
    assert_eq!(
        rotated_inventory.settings.effective_content["apiToken"],
        "••••••••"
    );
    assert_eq!(
        plan.mutations[0].content.as_ref().unwrap()["projectToken"],
        "project-secret"
    );
    let model = inventory
        .settings
        .fields
        .iter()
        .find(|field| field.path == "/model")
        .unwrap();
    assert_eq!(model.declarations.len(), 3);
    assert_eq!(model.value, "local");
    assert!(inventory
        .settings
        .layers
        .iter()
        .find(|layer| layer.logical_id == "user-settings")
        .is_some_and(|layer| !layer.editable));
    assert_eq!(inventory.settings.editable_targets.len(), 2);
    assert!(inventory
        .skills
        .resources
        .iter()
        .any(|resource| resource.logical_id == "project-skill"));
    let plugin = inventory
        .plugins
        .resources
        .iter()
        .find(|resource| resource.logical_id == "demo")
        .unwrap();
    assert_eq!(plugin.provenance.declarations.len(), 3);
    assert_eq!(
        plugin.effective_state,
        ad_lib::agents::EffectiveResourceState::Disabled
    );
}

#[test]
#[serial_test::serial(home_env)]
fn codex_inventory_uses_manifest_provenance_instead_of_generated_config() {
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("project");
    std::fs::create_dir_all(temp.path().join(".codex")).unwrap();
    std::fs::create_dir_all(project.join(".codex")).unwrap();
    std::fs::write(
        temp.path().join(".codex/config.toml"),
        "model = \"base\"\napi_key = \"secret-value\"\n[plugins.\"demo@market\"]\nenabled = true\n",
    )
    .unwrap();
    std::fs::write(
        project.join(".codex/config.toml"),
        "approval_policy = \"on-request\"\n",
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
    let unprepared = inspect_project_workspace_inventory(&base.id, &project).unwrap();
    assert!(unprepared.workspace.project_runtime.is_none());
    assert!(unprepared.settings.editable_targets.is_empty());

    let runtime = ProjectCodexRuntime::derive(&base, &project).unwrap();
    std::fs::create_dir_all(&runtime.runtime_home).unwrap();
    std::fs::write(
        runtime.runtime_home.join("config.toml"),
        "model = \"project\"\napi_key = \"secret-value\"\n[plugins.\"demo@market\"]\nenabled = false\n",
    )
    .unwrap();
    let manifest = ProjectCodexRuntimeManifest {
        schema_version: PROJECT_CODEX_RUNTIME_MANIFEST_SCHEMA_VERSION,
        applied_inherit_base_config: true,
        applied_profile_id: None,
        project_overlay: ProjectPluginOverlay {
            marketplaces: BTreeMap::new(),
            enabled_plugins: BTreeMap::from([("demo@market".into(), false)]),
        },
        project_settings_keys: BTreeSet::from(["model".into()]),
    };
    std::fs::create_dir_all(runtime.manifest_path().parent().unwrap()).unwrap();
    std::fs::write(
        runtime.manifest_path(),
        render_project_codex_runtime_manifest(&manifest).unwrap(),
    )
    .unwrap();
    persist_project_codex_runtime(&runtime).unwrap();

    let inventory = inspect_project_workspace_inventory(&base.id, &project).unwrap();

    let editable = &inventory.settings.editable_targets[0];
    let mut proposed = inventory
        .settings
        .layers
        .iter()
        .find(|layer| layer.declaration.key == editable.declaration_key)
        .unwrap()
        .content
        .clone();
    proposed["model"] = serde_json::Value::String("next".into());
    let plan = builtin_registry()
        .adapter("codex")
        .unwrap()
        .settings()
        .unwrap()
        .plan_edit(
            &AgentContext {
                installation_id: inventory.workspace.effective_installation_id.clone(),
                project_path: Some(inventory.workspace.canonical_project_path.clone()),
            },
            SettingsEdit {
                resource: editable.resource.clone(),
                media_type: editable.media_type.clone(),
                content: proposed,
            },
        )
        .unwrap();

    restore_env(previous_home, previous_codex_home);

    assert!(inventory.workspace.project_runtime.is_some());
    assert_eq!(inventory.settings.effective_content["model"], "project");
    assert_eq!(inventory.settings.effective_content["api_key"], "••••••••");
    assert_eq!(inventory.settings.editable_targets.len(), 1);
    let rendered = plan
        .mutations
        .iter()
        .find(|mutation| mutation.resource.logical_id == "runtime-config")
        .and_then(|mutation| mutation.content.as_ref())
        .and_then(serde_json::Value::as_str)
        .unwrap();
    assert!(rendered.contains("model = \"next\""));
    assert!(rendered.contains("api_key = \"secret-value\""));
    assert!(!rendered.contains("••••••••"));
    assert!(inventory
        .settings
        .layers
        .iter()
        .any(|layer| layer.declaration.source_id == "runtime-manifest"));
    assert!(!inventory
        .settings
        .layers
        .iter()
        .any(|layer| layer.declaration.source_id == "generated-config"));
    let plugin = inventory
        .plugins
        .resources
        .iter()
        .find(|resource| resource.logical_id == "demo@market")
        .unwrap();
    assert_eq!(plugin.provenance.declarations.len(), 2);
    assert_eq!(
        plugin.effective_state,
        ad_lib::agents::EffectiveResourceState::Disabled
    );
}
