use ad_lib::agents::{
    builtin_registry, AcknowledgementRequirement, AgentContext, AgentErrorCode,
    ClaudeToCodexOptions, ClaudeToCodexRoute, CodexPermissionPreset, ConversionRoute,
    ExecutionEngine, OperationReceipt, OperationStatus, PlanAcknowledgement,
    PlanAcknowledgementCode, PlanRiskLevel, PlanStore, ProjectCodexRuntime, ReceiptId,
    ResourceKind, ResourceScope,
};
use serial_test::serial;
use std::collections::BTreeSet;

fn apply_rollback(
    receipt_id: &ReceiptId,
) -> Result<OperationReceipt, Box<ad_lib::agents::AgentError>> {
    let plans = PlanStore::default();
    let plan = ExecutionEngine
        .preview_rollback(receipt_id, &plans)
        .map_err(Box::new)?;
    ExecutionEngine
        .apply_acknowledged(
            &plan.id,
            &plans,
            &[PlanAcknowledgement {
                code: PlanAcknowledgementCode::RollbackApply,
                accepted: true,
            }],
        )
        .map_err(Box::new)
}

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
    assert_eq!(unconfirmed.code, AgentErrorCode::ConfirmationRequired);
    assert_eq!(std::fs::read(&target_path).unwrap(), target_bytes);

    let applied = ExecutionEngine.apply_confirmed(&plan_id, &plans).unwrap();
    assert_eq!(applied.status, OperationStatus::Complete);
    assert!(!applied.post_apply_states.is_empty());
    assert!(applied.manifest_digest.is_some());
    assert_eq!(std::fs::read(&source_path).unwrap(), source_bytes);
    assert_ne!(std::fs::read(&target_path).unwrap(), target_bytes);

    let rollback = apply_rollback(&applied.id).unwrap();
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

    let error = apply_rollback(&applied.id).unwrap_err();

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
    let shared_source = br#"{
      "model":"project-shared",
      "model_reasoning_effort":"medium",
      "enabledPlugins":{"shared-plugin":true,"overridden":true}
    }"#;
    let local_source = br#"{
      "model":"project-local",
      "model_verbosity":"low",
      "enabledPlugins":{"project-only@marketplace":true,"overridden":false},
      "extraKnownMarketplaces":{"team-marketplace":{"source":"internal"},"tools":{"source":"github"}},
      "permissions":{"allow":["Read"],"ask":["Bash"]}
    }"#;
    let project_target = b"project_only = true\n";
    let user_source_path = claude_home.join("settings.json");
    let user_target_path = codex_home.join("config.toml");
    let shared_source_path = project.join(".claude/settings.json");
    let local_source_path = project.join(".claude/settings.local.json");
    let project_target_path = project.join(".codex/config.toml");
    std::fs::write(&user_source_path, user_source).unwrap();
    std::fs::write(&user_target_path, user_target).unwrap();
    std::fs::write(codex_home.join("auth.json"), "shared-login").unwrap();
    std::fs::write(&shared_source_path, shared_source).unwrap();
    std::fs::write(&local_source_path, local_source).unwrap();
    std::fs::write(&project_target_path, project_target).unwrap();
    create_project_skill(home.path(), &project, "review");
    create_user_skill(home.path(), &claude_home, "inherited");

    let previous_home = std::env::var("AD_HOME").ok();
    let previous_codex_home = std::env::var("CODEX_HOME").ok();
    std::env::set_var("AD_HOME", home.path());
    std::env::remove_var("CODEX_HOME");

    let canonical_project = std::fs::canonicalize(&project)
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let (source, target) = contexts(Some(canonical_project));
    let runtime_config_path = project_runtime_config_path(&target);
    let route = ClaudeToCodexRoute;
    let plans = PlanStore::default();
    let route_plan = route
        .preview_with_options(
            &source,
            &target,
            &ClaudeToCodexOptions {
                target_model: Some("project-local".into()),
                permission_preset: None,
                confirmed_skill_ids: BTreeSet::from(["inherited".into(), "review".into()]),
                profile_id: None,
                inherit_base_config: true,
                safe_subset: false,
            },
        )
        .unwrap();

    assert!(!route_plan.artifacts.is_empty());
    assert!(route_plan
        .artifacts
        .iter()
        .any(|artifact| artifact.source.resource.scope == ResourceScope::User));
    assert!(route_plan
        .plan
        .read_set
        .iter()
        .filter(|precondition| precondition.resource.installation_id == source.installation_id)
        .all(|precondition| precondition.write_policy == ad_lib::agents::WritePolicy::ReadOnly));
    assert!(route_plan
        .plan
        .read_set
        .iter()
        .any(|precondition| precondition.resource.scope == ResourceScope::User));
    assert!(route_plan
        .plan
        .mutations
        .iter()
        .all(|mutation| mutation.resource.scope == ResourceScope::Project));
    assert!(!route_plan
        .plan
        .mutations
        .iter()
        .any(|mutation| mutation.resource.kind == ResourceKind::Skills));
    let inherited_skill = route_plan
        .artifacts
        .iter()
        .find(|artifact| artifact.id == "skill:inherited")
        .unwrap();
    assert_eq!(inherited_skill.source.resource.scope, ResourceScope::User);
    assert_eq!(
        inherited_skill.disposition,
        ad_lib::agents::ArtifactDisposition::Unsupported
    );
    assert_eq!(
        inherited_skill.target.as_ref().unwrap().resource.scope,
        ResourceScope::Project
    );
    let plugin = route_plan
        .artifacts
        .iter()
        .find(|artifact| artifact.id == "plugin:project-only@marketplace")
        .unwrap();
    assert_eq!(
        plugin.disposition,
        ad_lib::agents::ArtifactDisposition::Unsupported
    );
    let shared_plugin = route_plan
        .artifacts
        .iter()
        .find(|artifact| artifact.id == "plugin:shared-plugin")
        .unwrap();
    assert!(shared_plugin
        .source
        .location
        .path
        .ends_with("/.claude/settings.json"));
    let overridden_plugin = route_plan
        .artifacts
        .iter()
        .find(|artifact| artifact.id == "plugin:overridden")
        .unwrap();
    assert!(overridden_plugin
        .source
        .location
        .path
        .ends_with("/.claude/settings.local.json"));
    assert!(route_plan
        .artifacts
        .iter()
        .any(|artifact| artifact.id == "marketplace:team-marketplace"));
    assert!(route_plan
        .artifacts
        .iter()
        .any(|artifact| artifact.id == "marketplace:tools"));
    let rules = route_plan
        .artifacts
        .iter()
        .find(|artifact| artifact.id.ends_with(":permissions:rules"))
        .unwrap();
    assert_eq!(rules.item_count, Some(2));
    assert!(!route_plan
        .artifacts
        .iter()
        .any(|artifact| artifact.id.ends_with(":enabledPlugins")));

    let plan_id = route_plan.plan.id.clone();
    plans.insert_confirmation_required(route_plan.plan).unwrap();
    let applied = ExecutionEngine.apply_confirmed(&plan_id, &plans).unwrap();

    assert_eq!(applied.status, OperationStatus::Complete);
    assert_eq!(std::fs::read(&user_source_path).unwrap(), user_source);
    assert_eq!(std::fs::read(&user_target_path).unwrap(), user_target);
    assert_eq!(std::fs::read(&shared_source_path).unwrap(), shared_source);
    assert_eq!(std::fs::read(&local_source_path).unwrap(), local_source);
    assert_eq!(std::fs::read(&project_target_path).unwrap(), project_target);
    let converted = std::fs::read_to_string(&runtime_config_path).unwrap();
    assert!(converted.contains("model = \"project-local\""));
    assert!(converted.contains("model_reasoning_effort = \"medium\""));
    assert!(converted.contains("model_verbosity = \"low\""));
    assert!(converted.contains("sandbox_mode = \"read-only\""));
    let codex_skill = project.join(".agents/skills/review");
    let inherited_codex_skill = project.join(".agents/skills/inherited");
    assert!(!codex_skill.exists());
    assert!(!inherited_codex_skill.exists());

    let repeated = route
        .preview_with_options(
            &source,
            &target,
            &ClaudeToCodexOptions {
                target_model: Some("project-local".into()),
                permission_preset: None,
                confirmed_skill_ids: BTreeSet::from(["inherited".into(), "review".into()]),
                profile_id: None,
                inherit_base_config: true,
                safe_subset: false,
            },
        )
        .unwrap();
    assert!(repeated.plan.mutations.is_empty());
    assert!(repeated.artifacts.iter().any(|artifact| {
        artifact.disposition == ad_lib::agents::ArtifactDisposition::Unchanged
    }));

    let rollback = apply_rollback(&applied.id).unwrap();

    restore_env(previous_home, previous_codex_home);
    assert_eq!(rollback.status, OperationStatus::Complete);
    assert_eq!(std::fs::read(&project_target_path).unwrap(), project_target);
    assert_eq!(std::fs::read(&user_target_path).unwrap(), user_target);
    assert!(!runtime_config_path.exists());
    assert!(!codex_skill.exists());
    assert!(!inherited_codex_skill.exists());
    assert!(project.join(".claude/skills/review").is_symlink());
}

#[test]
#[serial(home_env)]
fn project_conversion_applies_explicit_model_and_permission_decisions() {
    let home = tempfile::tempdir().unwrap();
    let project = home.path().join("project");
    std::fs::create_dir_all(home.path().join(".claude")).unwrap();
    std::fs::create_dir_all(home.path().join(".codex")).unwrap();
    std::fs::create_dir_all(project.join(".claude")).unwrap();
    std::fs::create_dir_all(project.join(".codex")).unwrap();
    std::fs::write(
        project.join(".claude/settings.local.json"),
        br#"{
          "model":"opus[1m]",
          "maxContextTokens":250000,
          "permissions":{"permissions":{"defaultMode":"bypassPermissions"}}
        }"#,
    )
    .unwrap();
    std::fs::write(
        project.join(".codex/config.toml"),
        b"model = \"existing-codex-model\"\n",
    )
    .unwrap();
    std::fs::write(home.path().join(".codex/auth.json"), "shared-login").unwrap();

    let previous_home = std::env::var("AD_HOME").ok();
    let previous_codex_home = std::env::var("CODEX_HOME").ok();
    std::env::set_var("AD_HOME", home.path());
    std::env::remove_var("CODEX_HOME");

    let canonical_project = std::fs::canonicalize(&project)
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let (source, target) = contexts(Some(canonical_project));
    let safe_route_plan = ClaudeToCodexRoute.preview(&source, &target).unwrap();
    let safe_content = safe_route_plan
        .plan
        .mutations
        .iter()
        .find(|mutation| mutation.resource.logical_id == "runtime-config")
        .unwrap()
        .content
        .as_ref()
        .and_then(serde_json::Value::as_str)
        .unwrap()
        .parse::<toml::Value>()
        .unwrap();
    let model_artifact = safe_route_plan
        .artifacts
        .iter()
        .find(|artifact| artifact.id.ends_with(":model"))
        .unwrap();

    assert_eq!(
        model_artifact.disposition,
        ad_lib::agents::ArtifactDisposition::RequiresInput
    );
    assert!(safe_content.get("model").is_none());
    assert_eq!(
        safe_content["model_context_window"].as_integer(),
        Some(250_000)
    );
    assert!(safe_content.get("approval_policy").is_none());
    assert!(safe_content.get("sandbox_mode").is_none());

    let options = ClaudeToCodexOptions {
        target_model: Some("gpt-5.6-sol".into()),
        permission_preset: Some(CodexPermissionPreset::NeverDangerFullAccess),
        ..ClaudeToCodexOptions::default()
    };
    let route_plan = ClaudeToCodexRoute
        .preview_with_options(&source, &target, &options)
        .unwrap();
    assert_eq!(route_plan.summary.dangerous, 1);
    let content = route_plan
        .plan
        .mutations
        .iter()
        .find(|mutation| mutation.resource.logical_id == "runtime-config")
        .unwrap()
        .content
        .as_ref()
        .and_then(serde_json::Value::as_str)
        .unwrap()
        .parse::<toml::Value>()
        .unwrap();

    let plan_id = route_plan.plan.id.clone();
    let plans = PlanStore::default();
    plans
        .insert_with_acknowledgements(
            route_plan.plan,
            vec![
                AcknowledgementRequirement {
                    code: PlanAcknowledgementCode::ConversionApply,
                    risk: PlanRiskLevel::Confirmation,
                },
                AcknowledgementRequirement {
                    code: PlanAcknowledgementCode::DangerousPermissionExpansion,
                    risk: PlanRiskLevel::Dangerous,
                },
            ],
        )
        .unwrap();
    let missing = ExecutionEngine
        .apply_acknowledged(
            &plan_id,
            &plans,
            &[PlanAcknowledgement {
                code: PlanAcknowledgementCode::ConversionApply,
                accepted: true,
            }],
        )
        .unwrap_err();
    assert_eq!(missing.code, AgentErrorCode::ConfirmationRequired);
    let applied = ExecutionEngine
        .apply_acknowledged(
            &plan_id,
            &plans,
            &[
                PlanAcknowledgement {
                    code: PlanAcknowledgementCode::ConversionApply,
                    accepted: true,
                },
                PlanAcknowledgement {
                    code: PlanAcknowledgementCode::DangerousPermissionExpansion,
                    accepted: true,
                },
            ],
        )
        .unwrap();
    let rollback = apply_rollback(&applied.id).unwrap();

    restore_env(previous_home, previous_codex_home);
    assert_eq!(rollback.status, OperationStatus::Complete);
    assert_eq!(
        std::fs::read(project.join(".codex/config.toml")).unwrap(),
        b"model = \"existing-codex-model\"\n"
    );
    assert_eq!(content["model"].as_str(), Some("gpt-5.6-sol"));
    assert_eq!(content["model_context_window"].as_integer(), Some(250_000));
    assert_eq!(content["approval_policy"].as_str(), Some("never"));
    assert_eq!(content["sandbox_mode"].as_str(), Some("danger-full-access"));
}

fn contexts(project_path: Option<String>) -> (AgentContext, AgentContext) {
    let installations = builtin_registry().discover();
    let source_installation = installations
        .iter()
        .find(|installation| installation.agent_id.as_str() == "claude-code")
        .unwrap();
    let target_installation = installations
        .iter()
        .find(|installation| installation.agent_id.as_str() == "codex")
        .unwrap();
    let source = AgentContext {
        installation_id: source_installation.id.clone(),
        project_path: project_path.clone(),
    };
    let target_installation_id = project_path
        .as_deref()
        .map(|project_path| {
            ProjectCodexRuntime::derive(target_installation, std::path::Path::new(project_path))
                .unwrap()
                .runtime_installation_id
        })
        .unwrap_or_else(|| target_installation.id.clone());
    let target = AgentContext {
        installation_id: target_installation_id,
        project_path,
    };
    (source, target)
}

fn project_runtime_config_path(context: &AgentContext) -> std::path::PathBuf {
    std::path::PathBuf::from(
        context
            .installation_id
            .as_str()
            .strip_prefix("codex:")
            .unwrap(),
    )
    .join("config.toml")
}

fn create_project_skill(home: &std::path::Path, project: &std::path::Path, name: &str) {
    let source_root = home.join(".ad/skill-library/local");
    let skill = source_root.join(name);
    std::fs::create_dir_all(&skill).unwrap();
    std::fs::write(
        skill.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: Project skill\n---\n"),
    )
    .unwrap();
    let state = home.join(".ad/state");
    std::fs::create_dir_all(&state).unwrap();
    std::fs::write(
        state.join("skill_sources.json"),
        serde_json::to_vec(&serde_json::json!([{
            "id": "local",
            "sourceType": "local",
            "url": source_root,
            "autoUpdate": false,
            "addedAt": "2026-07-16T00:00:00Z"
        }]))
        .unwrap(),
    )
    .unwrap();
    let skills = project.join(".claude/skills");
    std::fs::create_dir_all(&skills).unwrap();
    std::os::unix::fs::symlink(skill, skills.join(name)).unwrap();
}

fn create_user_skill(home: &std::path::Path, claude_home: &std::path::Path, name: &str) {
    let skill = home.join(".ad/skill-library/local").join(name);
    std::fs::create_dir_all(&skill).unwrap();
    std::fs::write(
        skill.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: Inherited user skill\n---\n"),
    )
    .unwrap();
    let skills = claude_home.join("skills");
    std::fs::create_dir_all(&skills).unwrap();
    std::os::unix::fs::symlink(skill, skills.join(name)).unwrap();
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
