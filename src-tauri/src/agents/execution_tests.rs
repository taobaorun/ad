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
        serde_json::from_slice::<serde_json::Value>(&std::fs::read(shared).unwrap()).unwrap()
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
}

#[test]
#[serial_test::serial(home_env)]
fn backup_failure_happens_before_any_target_write() {
    let (_temp, store, plan_id, shared, local) = setup_two_file_plan();
    let faults = FailAt::new([ExecutionStep::Backup(1)]);

    let error = ExecutionEngine
        .apply_with_faults(&plan_id, &store, &faults)
        .unwrap_err();

    assert_eq!(error.code, AgentErrorCode::Io);
    assert_eq!(std::fs::read(shared).unwrap(), br#"{"model":"old"}"#);
    assert_eq!(
        std::fs::read(local).unwrap(),
        br#"{"permissions":{"allow":[]}}"#
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
    assert_eq!(std::fs::read(shared).unwrap(), br#"{"model":"old"}"#);
    assert_eq!(
        std::fs::read(local).unwrap(),
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
        serde_json::from_slice::<serde_json::Value>(&std::fs::read(shared).unwrap()).unwrap()
            ["model"],
        "new"
    );
    assert_eq!(
        std::fs::read(local).unwrap(),
        br#"{"permissions":{"allow":[]}}"#
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
