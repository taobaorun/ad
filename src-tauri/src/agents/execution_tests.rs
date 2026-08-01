use chrono::{Duration, Utc};

use super::*;

fn setup_two_file_plan() -> (
    tempfile::TempDir,
    PlanStore,
    PlanId,
    std::path::PathBuf,
    std::path::PathBuf,
) {
    let temp = tempfile::tempdir().unwrap();
    std::env::set_var("AD_HOME", temp.path());
    std::env::remove_var("CODEX_HOME");
    std::fs::create_dir_all(temp.path().join(".claude")).unwrap();
    let project = temp.path().join("project");
    std::fs::create_dir_all(project.join(".claude")).unwrap();
    let project = std::fs::canonicalize(project).unwrap();
    let shared = project.join(".claude/settings.json");
    let local = project.join(".claude/settings.local.json");
    let shared_before = br#"{"model":"old"}"#;
    let local_before = br#"{"permissions":{"allow":[]}}"#;
    std::fs::write(&shared, shared_before).unwrap();
    std::fs::write(&local, local_before).unwrap();

    let installation = builtin_registry()
        .discover()
        .into_iter()
        .find(|item| item.agent_id.as_str() == "claude-code")
        .unwrap();
    let context = AgentContext {
        installation_id: installation.id,
        project_path: Some(project.to_string_lossy().into_owned()),
    };
    let resource = |logical_id: &str| ResourceRef {
        installation_id: context.installation_id.clone(),
        project_path: context.project_path.clone(),
        kind: ResourceKind::Settings,
        scope: ResourceScope::Project,
        logical_id: logical_id.into(),
    };
    let plan_id = PlanId::from("plan-two-files");
    let plan = MutationPlan {
        id: plan_id.clone(),
        agent_id: AgentId::from("claude-code"),
        context: context.clone(),
        read_set: Vec::new(),
        mutations: vec![
            PlannedMutation {
                resource: resource("project-shared"),
                kind: MutationKind::Replace,
                expected_digest: Some(ContentDigest::sha256(shared_before)),
                media_type: "application/json".into(),
                content: Some(serde_json::json!({"model": "new"})),
            },
            PlannedMutation {
                resource: resource("project-local"),
                kind: MutationKind::Replace,
                expected_digest: Some(ContentDigest::sha256(local_before)),
                media_type: "application/json".into(),
                content: Some(serde_json::json!({"permissions": {"allow": ["Read"]}})),
            },
        ],
        expires_at: Utc::now() + Duration::minutes(5),
    };
    let store = PlanStore::default();
    store.insert(plan).unwrap();
    (temp, store, plan_id, shared, local)
}

#[test]
#[serial_test::serial(home_env)]
fn execution_backs_up_all_targets_before_atomic_writes() {
    let (temp, store, plan_id, shared, local) = setup_two_file_plan();

    let receipt = ExecutionEngine.apply(&plan_id, &store).unwrap();

    assert_eq!(receipt.status, OperationStatus::Complete);
    assert_eq!(receipt.applied_resources.len(), 2);
    assert_eq!(receipt.backup_paths.len(), 2);
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&std::fs::read(&shared).unwrap()).unwrap()
            ["model"],
        "new"
    );
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&std::fs::read(local).unwrap()).unwrap()
            ["permissions"]["allow"][0],
        "Read"
    );
    assert!(receipt
        .backup_paths
        .iter()
        .all(|path| std::path::Path::new(path).is_file()));
    let operation_dirs = std::fs::read_dir(temp.path().join(".ad/backups/operations"))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(operation_dirs.len(), 1);
    assert!(operation_dirs[0].path().join("manifest.json").is_file());
    assert!(temp
        .path()
        .join(".ad/history/operations")
        .join(format!("{}.json", receipt.id))
        .is_file());
    let journals = std::fs::read_dir(temp.path().join(".ad/state/operation-journals"))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(journals.len(), 1);
    let journal_bytes = std::fs::read(journals[0].path()).unwrap();
    let journal: serde_json::Value = serde_json::from_slice(&journal_bytes).unwrap();
    assert_eq!(journal["schemaVersion"], 1);
    assert_eq!(journal["operationId"], plan_id.as_str());
    assert_eq!(journal["plannedReceiptId"], receipt.id.as_str());
    assert_eq!(journal["state"], "committed");
    assert!(!String::from_utf8(journal_bytes)
        .unwrap()
        .contains("permissions"));
}

#[test]
#[serial_test::serial(home_env)]
fn applying_journal_failure_happens_before_target_writes() {
    let (temp, store, plan_id, shared, local) = setup_two_file_plan();
    let faults = FailAt::new([ExecutionStep::PersistJournalApplying]);

    let error = ExecutionEngine
        .apply_with_faults(&plan_id, &store, &faults)
        .unwrap_err();

    assert_eq!(error.code, AgentErrorCode::Io);
    assert_eq!(std::fs::read(shared).unwrap(), br#"{"model":"old"}"#);
    assert_eq!(
        std::fs::read(local).unwrap(),
        br#"{"permissions":{"allow":[]}}"#
    );
    let journal_path = std::fs::read_dir(temp.path().join(".ad/state/operation-journals"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let journal: serde_json::Value =
        serde_json::from_slice(&std::fs::read(journal_path).unwrap()).unwrap();
    assert_eq!(journal["state"], "prepared");
}

#[test]
#[serial_test::serial(home_env)]
fn backup_failure_happens_before_any_target_write() {
    let (temp, store, plan_id, shared, local) = setup_two_file_plan();
    let faults = FailAt::new([ExecutionStep::Backup(1)]);

    let error = ExecutionEngine
        .apply_with_faults(&plan_id, &store, &faults)
        .unwrap_err();

    assert_eq!(error.code, AgentErrorCode::Io);
    assert_eq!(std::fs::read(shared).unwrap(), br#"{"model":"old"}"#);
    assert_eq!(
        std::fs::read(&local).unwrap(),
        br#"{"permissions":{"allow":[]}}"#
    );
    assert_eq!(
        std::fs::read_dir(temp.path().join(".ad/backups/operations"))
            .unwrap()
            .count(),
        0
    );
}

#[test]
#[serial_test::serial(home_env)]
fn second_write_failure_compensates_the_first_write() {
    let (_temp, store, plan_id, shared, local) = setup_two_file_plan();
    let faults = FailAt::new([ExecutionStep::Apply(1)]);

    let receipt = ExecutionEngine
        .apply_with_faults(&plan_id, &store, &faults)
        .unwrap();

    assert_eq!(receipt.status, OperationStatus::Compensated);
    assert_eq!(std::fs::read(&shared).unwrap(), br#"{"model":"old"}"#);
    assert_eq!(
        std::fs::read(&local).unwrap(),
        br#"{"permissions":{"allow":[]}}"#
    );
}

#[test]
#[serial_test::serial(home_env)]
fn compensation_failure_returns_a_partial_receipt() {
    let (_temp, store, plan_id, shared, local) = setup_two_file_plan();
    let faults = FailAt::new([ExecutionStep::Apply(1), ExecutionStep::Compensate(0)]);

    let receipt = ExecutionEngine
        .apply_with_faults(&plan_id, &store, &faults)
        .unwrap();

    assert_eq!(receipt.status, OperationStatus::PartialFailure);
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&std::fs::read(&shared).unwrap()).unwrap()
            ["model"],
        "new"
    );
    assert_eq!(
        std::fs::read(&local).unwrap(),
        br#"{"permissions":{"allow":[]}}"#
    );
    let journal_path = std::fs::read_dir(_temp.path().join(".ad/state/operation-journals"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let journal: serde_json::Value =
        serde_json::from_slice(&std::fs::read(journal_path).unwrap()).unwrap();
    assert_eq!(journal["state"], "repair_required");

    let rollback = ExecutionEngine.rollback(&receipt.id).unwrap();

    assert_eq!(rollback.status, OperationStatus::Complete);
    assert_eq!(std::fs::read(&shared).unwrap(), br#"{"model":"old"}"#);
    assert_eq!(
        std::fs::read(&local).unwrap(),
        br#"{"permissions":{"allow":[]}}"#
    );
}

#[test]
#[serial_test::serial(home_env)]
fn rollback_rejects_a_tampered_file_backup() {
    let (_temp, store, plan_id, shared, _local) = setup_two_file_plan();
    let receipt = ExecutionEngine.apply(&plan_id, &store).unwrap();
    std::fs::write(&receipt.backup_paths[0], br#"{"model":"tampered"}"#).unwrap();

    let error = ExecutionEngine.rollback(&receipt.id).unwrap_err();

    assert_eq!(error.code, AgentErrorCode::ResourceChanged);
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&std::fs::read(shared).unwrap()).unwrap()
            ["model"],
        "new"
    );
}

#[test]
#[serial_test::serial(home_env)]
fn rollback_rejects_a_tampered_directory_backup() {
    let temp = tempfile::tempdir().unwrap();
    std::env::set_var("AD_HOME", temp.path());
    std::env::remove_var("CODEX_HOME");
    let base_home = temp.path().join(".codex");
    let project = temp.path().join("project");
    std::fs::create_dir_all(&base_home).unwrap();
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(base_home.join("config.toml"), "model = \"base\"\n").unwrap();

    let registry = builtin_registry();
    let base = registry
        .discover()
        .into_iter()
        .find(|installation| installation.agent_id.as_str() == "codex")
        .unwrap();
    let runtime = ProjectCodexRuntime::derive(&base, &project).unwrap();
    let target = runtime.runtime_home.join("plugins/cache/team/demo/1.0.0");
    let source = temp.path().join("stage/demo");
    std::fs::create_dir_all(&target).unwrap();
    std::fs::create_dir_all(&source).unwrap();
    std::fs::write(target.join("version.txt"), "old").unwrap();
    std::fs::write(source.join("version.txt"), "new").unwrap();
    let context = AgentContext {
        installation_id: runtime.runtime_installation_id.clone(),
        project_path: Some(runtime.project_path.clone()),
    };
    let resource = ResourceRef {
        installation_id: context.installation_id.clone(),
        project_path: context.project_path.clone(),
        kind: ResourceKind::Plugins,
        scope: ResourceScope::Project,
        logical_id: "package:team:demo:1.0.0".into(),
    };
    let plan_id = PlanId::from("plan-directory-backup");
    let plan = MutationPlan {
        id: plan_id.clone(),
        agent_id: AgentId::from("codex"),
        context: context.clone(),
        read_set: Vec::new(),
        mutations: vec![PlannedMutation {
            resource,
            kind: MutationKind::Replace,
            expected_digest: Some(directory_tree_digest(&target).unwrap()),
            media_type: "application/vnd.ad.directory".into(),
            content: Some(serde_json::json!({
                "path": source,
                "digest": directory_tree_digest(&source).unwrap(),
            })),
        }],
        expires_at: Utc::now() + Duration::minutes(5),
    };
    let store = PlanStore::default();
    store.insert(plan).unwrap();
    let receipt = ExecutionEngine.apply(&plan_id, &store).unwrap();
    std::fs::write(
        std::path::Path::new(&receipt.backup_paths[0]).join("version.txt"),
        "tampered",
    )
    .unwrap();

    let error = ExecutionEngine.rollback(&receipt.id).unwrap_err();

    assert_eq!(error.code, AgentErrorCode::ResourceChanged);
    assert_eq!(
        std::fs::read_to_string(target.join("version.txt")).unwrap(),
        "new"
    );
}

#[test]
#[serial_test::serial(home_env)]
fn partial_unregistered_project_runtime_can_be_rolled_back() {
    let temp = tempfile::tempdir().unwrap();
    std::env::set_var("AD_HOME", temp.path());
    std::env::remove_var("CODEX_HOME");
    let base_home = temp.path().join(".codex");
    let project = temp.path().join("project");
    std::fs::create_dir_all(&base_home).unwrap();
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(base_home.join("config.toml"), "model = \"base\"\n").unwrap();

    let registry = builtin_registry();
    let base = registry
        .discover()
        .into_iter()
        .find(|installation| installation.agent_id.as_str() == "codex")
        .unwrap();
    let runtime = ProjectCodexRuntime::derive(&base, &project).unwrap();
    let context = AgentContext {
        installation_id: runtime.runtime_installation_id.clone(),
        project_path: Some(runtime.project_path.clone()),
    };
    let resource = |kind, logical_id: &str| ResourceRef {
        installation_id: context.installation_id.clone(),
        project_path: context.project_path.clone(),
        kind,
        scope: ResourceScope::Project,
        logical_id: logical_id.into(),
    };
    let plan_id = PlanId::from("plan-unregistered-runtime-partial");
    let plan = MutationPlan {
        id: plan_id.clone(),
        agent_id: AgentId::from("codex"),
        context: context.clone(),
        read_set: Vec::new(),
        mutations: vec![
            PlannedMutation {
                resource: resource(ResourceKind::Settings, "runtime-config"),
                kind: MutationKind::Create,
                expected_digest: None,
                media_type: "application/toml".into(),
                content: Some(serde_json::Value::String("model = \"project\"\n".into())),
            },
            PlannedMutation {
                resource: resource(ResourceKind::Plugins, "runtime-manifest"),
                kind: MutationKind::Create,
                expected_digest: None,
                media_type: "application/json".into(),
                content: Some(serde_json::json!({"schemaVersion": 1})),
            },
        ],
        expires_at: Utc::now() + Duration::minutes(5),
    };
    let store = PlanStore::default();
    store.insert(plan).unwrap();
    let receipt = ExecutionEngine
        .apply_with_faults(
            &plan_id,
            &store,
            &FailAt::new([ExecutionStep::Apply(1), ExecutionStep::Compensate(0)]),
        )
        .unwrap();
    assert_eq!(receipt.status, OperationStatus::PartialFailure);
    assert!(runtime.runtime_home.join("config.toml").is_file());
    assert!(runtime_for_installation(&runtime.runtime_installation_id).is_none());

    let rollback = ExecutionEngine.rollback(&receipt.id).unwrap();

    assert_eq!(rollback.status, OperationStatus::Complete);
    assert!(!runtime.runtime_home.join("config.toml").exists());
}

#[test]
#[serial_test::serial(home_env)]
fn receipt_persistence_failure_compensates_all_writes() {
    let (temp, store, plan_id, shared, local) = setup_two_file_plan();
    let faults = FailAt::new([ExecutionStep::PersistReceipt]);

    let error = ExecutionEngine
        .apply_with_faults(&plan_id, &store, &faults)
        .unwrap_err();

    assert_eq!(error.code, AgentErrorCode::Io);
    assert!(error.message.contains("applied changes were compensated"));
    assert_eq!(std::fs::read(shared).unwrap(), br#"{"model":"old"}"#);
    assert_eq!(
        std::fs::read(local).unwrap(),
        br#"{"permissions":{"allow":[]}}"#
    );
    let journal_path = std::fs::read_dir(temp.path().join(".ad/state/operation-journals"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let journal: serde_json::Value =
        serde_json::from_slice(&std::fs::read(journal_path).unwrap()).unwrap();
    assert_eq!(journal["state"], "compensated");
}

#[test]
#[serial_test::serial(home_env)]
fn failure_receipt_persistence_still_records_compensation() {
    let (temp, store, plan_id, shared, local) = setup_two_file_plan();
    let faults = FailAt::new([ExecutionStep::Apply(1), ExecutionStep::PersistReceipt]);

    let error = ExecutionEngine
        .apply_with_faults(&plan_id, &store, &faults)
        .unwrap_err();

    assert_eq!(error.code, AgentErrorCode::Io);
    assert_eq!(std::fs::read(shared).unwrap(), br#"{"model":"old"}"#);
    assert_eq!(
        std::fs::read(local).unwrap(),
        br#"{"permissions":{"allow":[]}}"#
    );
    let journal_path = std::fs::read_dir(temp.path().join(".ad/state/operation-journals"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let journal: serde_json::Value =
        serde_json::from_slice(&std::fs::read(journal_path).unwrap()).unwrap();
    assert_eq!(journal["state"], "compensated");
}

#[test]
#[serial_test::serial(home_env)]
fn receipt_construction_failure_compensates_all_writes_and_cleans_backups() {
    let (temp, store, plan_id, shared, local) = setup_two_file_plan();
    let faults = FailAt::new([ExecutionStep::ConstructReceipt]);

    let error = ExecutionEngine
        .apply_with_faults(&plan_id, &store, &faults)
        .unwrap_err();

    assert_eq!(error.code, AgentErrorCode::Io);
    assert!(error.message.contains("could not be constructed"));
    assert!(error.message.contains("applied changes were compensated"));
    assert_eq!(std::fs::read(shared).unwrap(), br#"{"model":"old"}"#);
    assert_eq!(
        std::fs::read(local).unwrap(),
        br#"{"permissions":{"allow":[]}}"#
    );
    assert_eq!(
        std::fs::read_dir(temp.path().join(".ad/backups/operations"))
            .unwrap()
            .count(),
        0
    );
}

#[test]
#[serial_test::serial(home_env)]
fn execution_applies_an_allowlisted_skill_symlink_plan() {
    let temp = tempfile::tempdir().unwrap();
    std::env::set_var("AD_HOME", temp.path());
    std::env::remove_var("CODEX_HOME");
    std::fs::create_dir_all(temp.path().join(".claude")).unwrap();
    let source = temp.path().join("source/demo");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::write(source.join("SKILL.md"), "---\nname: demo\n---\n").unwrap();
    let registry = builtin_registry();
    let installation = registry
        .discover()
        .into_iter()
        .find(|item| item.agent_id.as_str() == "claude-code")
        .unwrap();
    let context = AgentContext {
        installation_id: installation.id,
        project_path: None,
    };
    let plan = registry
        .adapter("claude-code")
        .unwrap()
        .skills()
        .unwrap()
        .plan_install(
            &context,
            CollectionInstallRequest {
                logical_id: "demo".into(),
                source: serde_json::json!({"path": source}),
            },
        )
        .unwrap();
    let plan_id = plan.id.clone();
    let store = PlanStore::default();
    store.insert(plan).unwrap();

    let receipt = ExecutionEngine.apply(&plan_id, &store).unwrap();

    assert_eq!(receipt.status, OperationStatus::Complete);
    assert_eq!(
        std::fs::read_link(temp.path().join(".claude/skills/demo")).unwrap(),
        std::fs::canonicalize(source).unwrap()
    );
}

#[test]
#[serial_test::serial(home_env)]
fn execution_rejects_a_skill_source_changed_after_preview() {
    let temp = tempfile::tempdir().unwrap();
    std::env::set_var("AD_HOME", temp.path());
    std::env::remove_var("CODEX_HOME");
    std::fs::create_dir_all(temp.path().join(".claude")).unwrap();
    let source = temp.path().join("source/demo");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::write(source.join("SKILL.md"), "---\nname: demo\n---\n").unwrap();
    let registry = builtin_registry();
    let installation = registry
        .discover()
        .into_iter()
        .find(|item| item.agent_id.as_str() == "claude-code")
        .unwrap();
    let context = AgentContext {
        installation_id: installation.id,
        project_path: None,
    };
    let plan = registry
        .adapter("claude-code")
        .unwrap()
        .skills()
        .unwrap()
        .plan_install(
            &context,
            CollectionInstallRequest {
                logical_id: "demo".into(),
                source: serde_json::json!({"path": source}),
            },
        )
        .unwrap();
    let plan_id = plan.id.clone();
    let store = PlanStore::default();
    store.insert(plan).unwrap();
    std::fs::write(
        source.join("SKILL.md"),
        "---\nname: demo\n---\nChanged after preview.\n",
    )
    .unwrap();

    let error = ExecutionEngine.apply(&plan_id, &store).unwrap_err();

    assert_eq!(error.code, AgentErrorCode::ResourceChanged);
    assert!(!temp.path().join(".claude/skills/demo").exists());
}

#[test]
#[serial_test::serial(home_env)]
fn project_plugin_config_failure_restores_replaced_directories_before_activation() {
    let temp = tempfile::tempdir().unwrap();
    std::env::set_var("AD_HOME", temp.path());
    std::env::remove_var("CODEX_HOME");
    let base_home = temp.path().join(".codex");
    let project = temp.path().join("project");
    let marketplace_stage = temp
        .path()
        .join(".ad/staging/codex-plugin-conversion/demo/marketplace");
    let package_stage = temp
        .path()
        .join(".ad/staging/codex-plugin-conversion/demo/package");
    std::fs::create_dir_all(&base_home).unwrap();
    std::fs::create_dir_all(&project).unwrap();
    std::fs::create_dir_all(marketplace_stage.join(".agents/plugins")).unwrap();
    std::fs::create_dir_all(package_stage.join(".codex-plugin")).unwrap();
    std::fs::write(
        base_home.join("config.toml"),
        "cli_auth_credentials_store = \"file\"\n",
    )
    .unwrap();
    std::fs::write(base_home.join("auth.json"), "shared-login").unwrap();
    std::fs::write(
        marketplace_stage.join(".agents/plugins/marketplace.json"),
        r#"{"name":"team","revision":"new"}"#,
    )
    .unwrap();
    std::fs::write(
        package_stage.join(".codex-plugin/plugin.json"),
        r#"{"name":"demo","version":"1.0.0"}"#,
    )
    .unwrap();

    let registry = builtin_registry();
    let base = registry
        .discover()
        .into_iter()
        .find(|installation| {
            installation.root_path == std::fs::canonicalize(&base_home).unwrap().to_string_lossy()
        })
        .unwrap();
    let runtime = ProjectCodexRuntime::derive(&base, &project).unwrap();
    std::fs::create_dir_all(
        runtime
            .runtime_home
            .join(".tmp/marketplaces/team/.agents/plugins"),
    )
    .unwrap();
    std::fs::write(
        runtime
            .runtime_home
            .join(".tmp/marketplaces/team/.agents/plugins/marketplace.json"),
        r#"{"name":"team","revision":"old"}"#,
    )
    .unwrap();
    persist_project_codex_runtime(&runtime).unwrap();
    let context = AgentContext {
        installation_id: runtime.runtime_installation_id.clone(),
        project_path: Some(runtime.project_path.clone()),
    };
    let plan = registry
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
                        "lastRevision": "new",
                        "stagePath": marketplace_stage,
                    },
                    "package": {
                        "name": "demo",
                        "version": "1.0.0",
                        "stagePath": package_stage,
                    }
                }),
            },
        )
        .unwrap();
    assert_eq!(
        plan.mutations.last().unwrap().resource.logical_id,
        "demo@team"
    );
    let config_index = plan.mutations.len() - 1;
    let plan_id = plan.id.clone();
    let store = PlanStore::default();
    store.insert(plan).unwrap();

    let receipt = ExecutionEngine
        .apply_with_faults(
            &plan_id,
            &store,
            &FailAt::new([ExecutionStep::Apply(config_index)]),
        )
        .unwrap();

    assert_eq!(receipt.status, OperationStatus::Compensated);
    assert!(!runtime.runtime_home.join("config.toml").exists());
    assert!(!runtime.runtime_home.join("auth.json").exists());
    assert!(!runtime
        .runtime_home
        .join("plugins/cache/team/demo/1.0.0")
        .exists());
    assert_eq!(
        std::fs::read_to_string(
            runtime
                .runtime_home
                .join(".tmp/marketplaces/team/.agents/plugins/marketplace.json")
        )
        .unwrap(),
        r#"{"name":"team","revision":"old"}"#
    );
}
