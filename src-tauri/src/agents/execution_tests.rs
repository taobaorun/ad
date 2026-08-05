use chrono::{Duration, Utc};
use std::os::unix::fs::PermissionsExt;

use super::*;

fn apply_rollback(receipt_id: &ReceiptId) -> Result<OperationReceipt, Box<AgentError>> {
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

struct SwapAncestorAt {
    step: ExecutionStep,
    paths: std::sync::Mutex<Option<(std::path::PathBuf, std::path::PathBuf, std::path::PathBuf)>>,
}

impl FaultInjector for SwapAncestorAt {
    fn before_step(&self, step: ExecutionStep) {
        if step != self.step {
            return;
        }
        let Some((ancestor, moved, outside)) = self.paths.lock().unwrap().take() else {
            return;
        };
        std::fs::rename(&ancestor, moved).unwrap();
        std::os::unix::fs::symlink(outside, ancestor).unwrap();
    }

    fn should_fail(&self, _step: ExecutionStep) -> bool {
        false
    }
}

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

fn seed_existing_directory_ownership(plan: &MutationPlan, registry: &AdapterRegistry) {
    let state = super::execution_state::ExecutionState::open().unwrap();
    for mutation in &plan.mutations {
        let target = registry
            .resolve_resource(&plan.context, &mutation.resource)
            .unwrap();
        if !ownership_managed(&mutation.resource, target.storage())
            || target.storage() != ResourceStorage::Directory
        {
            continue;
        }
        let Ok(metadata) = std::fs::symlink_metadata(target.path()) else {
            continue;
        };
        if !metadata.is_dir() {
            continue;
        }
        let digest = directory_tree_digest(target.path()).unwrap();
        let receipt_id = ReceiptId::from("seed-owned-directory");
        let record_id = ownership_record_id(&mutation.resource);
        let record = ResourceOwnershipRecord {
            schema_version: RESOURCE_OWNERSHIP_SCHEMA_VERSION,
            id: record_id.clone(),
            workspace_key: ownership_workspace_key(&mutation.resource).unwrap(),
            resource: mutation.resource.clone(),
            target_id: PhysicalTargetId::for_resource(&mutation.resource),
            target_path: target.path().to_string_lossy().into_owned(),
            target_kind: ResourceStateKind::Directory,
            target_digest: digest.clone(),
            artifact_id: target.path().to_string_lossy().into_owned(),
            artifact_digest: digest,
            source_binding: None,
            creating_receipt_id: receipt_id.clone(),
            updated_by_receipt_id: receipt_id,
        };
        apply_ownership_changes(
            &state,
            &[ResourceOwnershipChange {
                kind: ResourceOwnershipChangeKind::Upsert,
                record_id,
                previous_record: None,
                record: Some(record),
            }],
        )
        .unwrap();
    }
}

#[test]
#[serial_test::serial(home_env)]
fn ownership_workspace_key_matches_the_backend_workspace_descriptor() {
    let temp = tempfile::tempdir().unwrap();
    std::env::set_var("AD_HOME", temp.path());
    std::env::remove_var("CODEX_HOME");
    std::fs::create_dir_all(temp.path().join(".claude")).unwrap();
    let project = temp.path().join("project");
    std::fs::create_dir_all(&project).unwrap();
    let canonical_project = std::fs::canonicalize(project).unwrap();
    let installation = builtin_registry()
        .discover()
        .into_iter()
        .find(|item| item.agent_id.as_str() == "claude-code")
        .unwrap();
    let descriptor = resolve_project_agent_workspace(&installation.id, &canonical_project).unwrap();
    let resource = ResourceRef {
        installation_id: installation.id,
        project_path: Some(canonical_project.to_string_lossy().into_owned()),
        kind: ResourceKind::Skills,
        scope: ResourceScope::Project,
        logical_id: "demo".into(),
    };

    assert_eq!(ownership_workspace_key(&resource).unwrap(), descriptor.key);
}

fn setup_project_claude_skill() -> (
    tempfile::TempDir,
    AdapterRegistry,
    AgentContext,
    std::path::PathBuf,
) {
    let temp = tempfile::tempdir().unwrap();
    std::env::set_var("AD_HOME", temp.path());
    std::env::remove_var("CODEX_HOME");
    std::fs::create_dir_all(temp.path().join(".claude")).unwrap();
    let project = temp.path().join("project");
    std::fs::create_dir_all(project.join(".claude/skills")).unwrap();
    let project = std::fs::canonicalize(project).unwrap();
    let source = temp.path().join(".ad/skill-library/catalog/demo");
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
        project_path: Some(project.to_string_lossy().into_owned()),
    };
    (temp, registry, context, source)
}

fn apply_project_claude_skill(
    registry: &AdapterRegistry,
    context: &AgentContext,
    source: &std::path::Path,
) -> OperationReceipt {
    let plan = registry
        .adapter("claude-code")
        .unwrap()
        .skills()
        .unwrap()
        .plan_install(
            context,
            CollectionInstallRequest {
                logical_id: "demo".into(),
                source: serde_json::json!({"path": source}),
            },
        )
        .unwrap();
    let plan_id = plan.id.clone();
    let plans = PlanStore::default();
    plans.insert(plan).unwrap();
    ExecutionEngine.apply(&plan_id, &plans).unwrap()
}

fn single_file_plan(
    agent_id: &str,
    context: AgentContext,
    resource: ResourceRef,
    before: &[u8],
    content: serde_json::Value,
    media_type: &str,
) -> (PlanStore, PlanId) {
    let plan_id = PlanId::from(uuid::Uuid::new_v4().to_string());
    let plan = MutationPlan {
        id: plan_id.clone(),
        agent_id: AgentId::from(agent_id),
        context,
        read_set: Vec::new(),
        mutations: vec![PlannedMutation {
            resource,
            kind: MutationKind::Replace,
            expected_digest: Some(ContentDigest::sha256(before)),
            media_type: media_type.into(),
            content: Some(content),
        }],
        expires_at: Utc::now() + Duration::minutes(5),
    };
    let store = PlanStore::default();
    store.insert(plan).unwrap();
    (store, plan_id)
}

#[test]
#[serial_test::serial(home_env)]
fn project_settings_ancestor_symlink_cannot_modify_outside_sentinel() {
    let temp = tempfile::tempdir().unwrap();
    std::env::set_var("AD_HOME", temp.path());
    std::env::remove_var("CODEX_HOME");
    std::fs::create_dir_all(temp.path().join(".claude")).unwrap();
    let project = temp.path().join("project");
    let outside = temp.path().join("outside");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    let sentinel = br#"{"model":"outside"}"#;
    std::fs::write(outside.join("settings.json"), sentinel).unwrap();
    std::os::unix::fs::symlink(&outside, project.join(".claude")).unwrap();
    let project = std::fs::canonicalize(project).unwrap();
    let installation = builtin_registry()
        .discover()
        .into_iter()
        .find(|item| item.agent_id.as_str() == "claude-code")
        .unwrap();
    let context = AgentContext {
        installation_id: installation.id,
        project_path: Some(project.to_string_lossy().into_owned()),
    };
    let resource = ResourceRef {
        installation_id: context.installation_id.clone(),
        project_path: context.project_path.clone(),
        kind: ResourceKind::Settings,
        scope: ResourceScope::Project,
        logical_id: "project-shared".into(),
    };
    let (store, plan_id) = single_file_plan(
        "claude-code",
        context,
        resource,
        sentinel,
        serde_json::json!({"model": "escaped"}),
        "application/json",
    );

    let error = ExecutionEngine.apply(&plan_id, &store).unwrap_err();

    assert_eq!(error.code, AgentErrorCode::PermissionDenied);
    assert_eq!(
        std::fs::read(outside.join("settings.json")).unwrap(),
        sentinel
    );
}

#[test]
#[serial_test::serial(home_env)]
fn user_agent_root_symlink_cannot_modify_outside_sentinel() {
    let temp = tempfile::tempdir().unwrap();
    std::env::set_var("AD_HOME", temp.path());
    std::env::remove_var("CODEX_HOME");
    std::fs::create_dir_all(temp.path().join(".claude")).unwrap();
    let outside = temp.path().join("outside-codex");
    std::fs::create_dir_all(&outside).unwrap();
    let sentinel = b"model = \"outside\"\n";
    std::fs::write(outside.join("config.toml"), sentinel).unwrap();
    std::os::unix::fs::symlink(&outside, temp.path().join(".codex")).unwrap();
    let installation = builtin_registry()
        .discover()
        .into_iter()
        .find(|item| item.agent_id.as_str() == "codex")
        .unwrap();
    let context = AgentContext {
        installation_id: installation.id,
        project_path: None,
    };
    let resource = ResourceRef {
        installation_id: context.installation_id.clone(),
        project_path: None,
        kind: ResourceKind::Settings,
        scope: ResourceScope::User,
        logical_id: "user-config".into(),
    };
    let (store, plan_id) = single_file_plan(
        "codex",
        context,
        resource,
        sentinel,
        serde_json::Value::String("model = \"escaped\"\n".into()),
        "application/toml",
    );

    let error = ExecutionEngine.apply(&plan_id, &store).unwrap_err();

    assert_eq!(error.code, AgentErrorCode::PermissionDenied);
    assert_eq!(
        std::fs::read(outside.join("config.toml")).unwrap(),
        sentinel
    );
}

#[test]
#[serial_test::serial(home_env)]
fn world_writable_ad_root_blocks_execution_before_target_write() {
    let (temp, store, plan_id, shared, _local) = setup_two_file_plan();
    let ad_root = temp.path().join(".ad");
    std::fs::create_dir_all(&ad_root).unwrap();
    std::fs::set_permissions(&ad_root, std::fs::Permissions::from_mode(0o777)).unwrap();

    let error = ExecutionEngine.apply(&plan_id, &store).unwrap_err();

    assert_eq!(error.code, AgentErrorCode::PermissionDenied);
    assert_eq!(std::fs::read(shared).unwrap(), br#"{"model":"old"}"#);
}

#[test]
#[serial_test::serial(home_env)]
fn symlinked_ad_backup_root_blocks_execution_before_target_write() {
    let (temp, store, plan_id, shared, _local) = setup_two_file_plan();
    let ad_root = temp.path().join(".ad");
    let outside = temp.path().join("outside-backups");
    std::fs::create_dir_all(&ad_root).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("sentinel"), b"outside").unwrap();
    std::os::unix::fs::symlink(&outside, ad_root.join("backups")).unwrap();

    let error = ExecutionEngine.apply(&plan_id, &store).unwrap_err();

    assert_eq!(error.code, AgentErrorCode::PermissionDenied);
    assert_eq!(std::fs::read(shared).unwrap(), br#"{"model":"old"}"#);
    assert_eq!(std::fs::read(outside.join("sentinel")).unwrap(), b"outside");
    assert_eq!(std::fs::read_dir(outside).unwrap().count(), 1);
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
    assert_eq!(
        journal["schemaVersion"],
        super::execution_journal::JOURNAL_SCHEMA_VERSION
    );
    assert_eq!(journal["operationId"], plan_id.as_str());
    assert_eq!(journal["plannedReceiptId"], receipt.id.as_str());
    assert_eq!(journal["state"], "committed");
    assert!(!String::from_utf8(journal_bytes)
        .unwrap()
        .contains("permissions"));
}

#[test]
#[serial_test::serial(home_env)]
fn rollback_is_a_fresh_acknowledged_plan_with_a_child_receipt() {
    let (_temp, store, plan_id, shared, local) = setup_two_file_plan();
    let applied = ExecutionEngine.apply(&plan_id, &store).unwrap();
    let rollback_store = PlanStore::default();

    let rollback_plan = ExecutionEngine
        .preview_rollback(&applied.id, &rollback_store)
        .unwrap();

    assert_eq!(rollback_plan.context, applied.context.clone().unwrap());
    assert_eq!(rollback_plan.required_acknowledgements.len(), 1);
    assert_eq!(
        rollback_plan.required_acknowledgements[0].code,
        PlanAcknowledgementCode::RollbackApply
    );
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&std::fs::read(&shared).unwrap()).unwrap()
            ["model"],
        "new"
    );
    let unacknowledged = ExecutionEngine
        .apply(&rollback_plan.id, &rollback_store)
        .unwrap_err();
    assert_eq!(unacknowledged.code, AgentErrorCode::ConfirmationRequired);

    let rollback = ExecutionEngine
        .apply_acknowledged(
            &rollback_plan.id,
            &rollback_store,
            &[PlanAcknowledgement {
                code: PlanAcknowledgementCode::RollbackApply,
                accepted: true,
            }],
        )
        .unwrap();

    assert_eq!(rollback.operation_kind, OperationKind::Rollback);
    assert_eq!(rollback.parent_receipt_id.as_ref(), Some(&applied.id));
    assert!(!rollback.rollback.available);
    assert_eq!(
        rollback.rollback.reason,
        Some(RollbackUnavailableReason::RollbackReceipt)
    );
    assert_eq!(std::fs::read(&shared).unwrap(), br#"{"model":"old"}"#);
    assert_eq!(
        std::fs::read(&local).unwrap(),
        br#"{"permissions":{"allow":[]}}"#
    );
    let history = list_operation_history(None, None, Some(20)).unwrap();
    let parent = history
        .iter()
        .filter_map(|entry| entry.receipt.as_ref())
        .find(|receipt| receipt.id == applied.id)
        .unwrap();
    assert_eq!(
        parent.rollback.reason,
        Some(RollbackUnavailableReason::AlreadyRolledBack)
    );
}

#[test]
#[serial_test::serial(home_env)]
fn legacy_receipts_cannot_create_inverse_plans() {
    let temp = tempfile::tempdir().unwrap();
    std::env::set_var("AD_HOME", temp.path());
    let history = temp.path().join(".ad/history/operations");
    std::fs::create_dir_all(&history).unwrap();
    let receipt_id = "00000000-0000-4000-8000-000000000001";
    std::fs::write(
        history.join(format!("{receipt_id}.json")),
        format!(
            r#"{{
            "id":"{receipt_id}",
            "planId":"legacy-plan",
            "status":"complete",
            "appliedResources":[],
            "backupPaths":[],
            "postApplyStates":[]
        }}"#
        ),
    )
    .unwrap();

    let error = ExecutionEngine
        .preview_rollback(&ReceiptId::from(receipt_id), &PlanStore::default())
        .unwrap_err();

    assert_eq!(error.code, AgentErrorCode::Unsupported);
}

#[test]
#[serial_test::serial(home_env)]
fn forged_rollback_receipts_cannot_create_inverse_plans() {
    let (temp, store, plan_id, _shared, _local) = setup_two_file_plan();
    let receipt = ExecutionEngine.apply(&plan_id, &store).unwrap();
    let receipt_path = temp
        .path()
        .join(".ad/history/operations")
        .join(format!("{}.json", receipt.id));
    let mut value =
        serde_json::from_slice::<serde_json::Value>(&std::fs::read(&receipt_path).unwrap())
            .unwrap();
    value["operationKind"] = serde_json::json!("rollback");
    value["rollback"] = serde_json::json!({"available": true});
    std::fs::write(&receipt_path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();

    let forged_kind = ExecutionEngine
        .preview_rollback(&receipt.id, &PlanStore::default())
        .unwrap_err();

    assert_eq!(forged_kind.code, AgentErrorCode::InvalidPlan);

    value["operationKind"] = serde_json::json!("apply");
    value["parentReceiptId"] = serde_json::json!(receipt.id);
    std::fs::write(&receipt_path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();

    let forged_parent = ExecutionEngine
        .preview_rollback(&receipt.id, &PlanStore::default())
        .unwrap_err();

    assert_eq!(forged_parent.code, AgentErrorCode::InvalidPlan);
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

    let rollback = apply_rollback(&receipt.id).unwrap();

    assert_eq!(rollback.status, OperationStatus::Complete);
    assert_eq!(std::fs::read(&shared).unwrap(), br#"{"model":"old"}"#);
    assert_eq!(
        std::fs::read(&local).unwrap(),
        br#"{"permissions":{"allow":[]}}"#
    );
}

#[test]
#[serial_test::serial(home_env)]
fn partial_plugin_failure_rolls_back_with_pre_apply_ownership() {
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
    let old_artifact = temp.path().join("staging/old");
    let new_artifact = temp.path().join("staging/new");
    std::fs::create_dir_all(&target).unwrap();
    std::fs::create_dir_all(&old_artifact).unwrap();
    std::fs::create_dir_all(&new_artifact).unwrap();
    std::fs::write(target.join("version.txt"), "old").unwrap();
    std::fs::write(old_artifact.join("version.txt"), "old").unwrap();
    std::fs::write(new_artifact.join("version.txt"), "new").unwrap();
    let config = runtime.runtime_home.join("config.toml");
    std::fs::write(&config, "model = \"old\"\n").unwrap();
    let context = AgentContext {
        installation_id: runtime.runtime_installation_id.clone(),
        project_path: Some(runtime.project_path.clone()),
    };
    let package_resource = ResourceRef {
        installation_id: context.installation_id.clone(),
        project_path: context.project_path.clone(),
        kind: ResourceKind::Plugins,
        scope: ResourceScope::Project,
        logical_id: "package:team:demo:1.0.0".into(),
    };
    let config_resource = ResourceRef {
        installation_id: context.installation_id.clone(),
        project_path: context.project_path.clone(),
        kind: ResourceKind::Plugins,
        scope: ResourceScope::Project,
        logical_id: "demo@team".into(),
    };
    let target_digest = directory_tree_digest(&target).unwrap();
    let old_artifact_digest = directory_tree_digest(&old_artifact).unwrap();
    let record_id = ownership_record_id(&package_resource);
    let seed_receipt = ReceiptId::from("seed-partial-plugin-ownership");
    let original_record = ResourceOwnershipRecord {
        schema_version: RESOURCE_OWNERSHIP_SCHEMA_VERSION,
        id: record_id.clone(),
        workspace_key: ownership_workspace_key(&package_resource).unwrap(),
        resource: package_resource.clone(),
        target_id: PhysicalTargetId::for_resource(&package_resource),
        target_path: target.to_string_lossy().into_owned(),
        target_kind: ResourceStateKind::Directory,
        target_digest: target_digest.clone(),
        artifact_id: old_artifact.to_string_lossy().into_owned(),
        artifact_digest: old_artifact_digest,
        source_binding: None,
        creating_receipt_id: seed_receipt.clone(),
        updated_by_receipt_id: seed_receipt,
    };
    let state = super::execution_state::ExecutionState::open().unwrap();
    apply_ownership_changes(
        &state,
        &[ResourceOwnershipChange {
            kind: ResourceOwnershipChangeKind::Upsert,
            record_id,
            previous_record: None,
            record: Some(original_record.clone()),
        }],
    )
    .unwrap();
    let plan_id = PlanId::from("partial-owned-plugin-plan");
    let plan = MutationPlan {
        id: plan_id.clone(),
        agent_id: AgentId::from("codex"),
        context,
        read_set: Vec::new(),
        mutations: vec![
            PlannedMutation {
                resource: package_resource.clone(),
                kind: MutationKind::Replace,
                expected_digest: Some(target_digest),
                media_type: "application/vnd.ad.directory".into(),
                content: Some(serde_json::json!({
                    "path": new_artifact,
                    "digest": directory_tree_digest(&new_artifact).unwrap(),
                })),
            },
            PlannedMutation {
                resource: config_resource,
                kind: MutationKind::Replace,
                expected_digest: Some(ContentDigest::sha256(b"model = \"old\"\n")),
                media_type: "application/toml".into(),
                content: Some(serde_json::Value::String("model = \"new\"\n".into())),
            },
        ],
        expires_at: Utc::now() + Duration::minutes(5),
    };
    let plans = PlanStore::default();
    plans.insert(plan).unwrap();
    let receipt = ExecutionEngine
        .apply_with_faults(
            &plan_id,
            &plans,
            &FailAt::new([ExecutionStep::Apply(1), ExecutionStep::Compensate(0)]),
        )
        .unwrap();

    assert_eq!(receipt.status, OperationStatus::PartialFailure);
    assert_eq!(
        std::fs::read_to_string(target.join("version.txt")).unwrap(),
        "new"
    );
    assert_eq!(
        load_ownership_record(&state, &package_resource).unwrap(),
        Some(original_record.clone())
    );

    let rollback = apply_rollback(&receipt.id).unwrap();
    let restored = load_ownership_record(&state, &package_resource)
        .unwrap()
        .unwrap();

    assert_eq!(rollback.status, OperationStatus::Complete);
    assert_eq!(
        std::fs::read_to_string(target.join("version.txt")).unwrap(),
        "old"
    );
    assert_eq!(
        restored.creating_receipt_id,
        original_record.creating_receipt_id
    );
    assert_eq!(restored.updated_by_receipt_id, rollback.id);
    assert_eq!(
        std::fs::read_to_string(config).unwrap(),
        "model = \"old\"\n"
    );
}

#[test]
#[serial_test::serial(home_env)]
fn partial_skill_create_rollback_does_not_remove_missing_ownership() {
    let (_temp, registry, context, source) = setup_project_claude_skill();
    let project = std::path::PathBuf::from(context.project_path.as_deref().unwrap());
    let settings_path = project.join(".claude/settings.json");
    let settings_before = br#"{"model":"old"}"#;
    std::fs::write(&settings_path, settings_before).unwrap();
    let mut plan = registry
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
    let skill_resource = plan.mutations[0].resource.clone();
    plan.mutations.push(PlannedMutation {
        resource: ResourceRef {
            installation_id: context.installation_id.clone(),
            project_path: context.project_path.clone(),
            kind: ResourceKind::Settings,
            scope: ResourceScope::Project,
            logical_id: "project-shared".into(),
        },
        kind: MutationKind::Replace,
        expected_digest: Some(ContentDigest::sha256(settings_before)),
        media_type: "application/json".into(),
        content: Some(serde_json::json!({"model": "new"})),
    });
    let plan_id = plan.id.clone();
    let plans = PlanStore::default();
    plans.insert(plan).unwrap();

    let receipt = ExecutionEngine
        .apply_with_faults(
            &plan_id,
            &plans,
            &FailAt::new([ExecutionStep::Apply(1), ExecutionStep::Compensate(0)]),
        )
        .unwrap();
    let state = super::execution_state::ExecutionState::open().unwrap();

    assert_eq!(receipt.status, OperationStatus::PartialFailure);
    assert!(project.join(".claude/skills/demo").is_symlink());
    assert!(load_ownership_record(&state, &skill_resource)
        .unwrap()
        .is_none());

    let rollback = apply_rollback(&receipt.id).unwrap();

    assert_eq!(rollback.status, OperationStatus::Complete);
    assert!(!project.join(".claude/skills/demo").exists());
    assert!(load_ownership_record(&state, &skill_resource)
        .unwrap()
        .is_none());
}

#[test]
#[serial_test::serial(home_env)]
fn rollback_rejects_a_tampered_file_backup() {
    let (_temp, store, plan_id, shared, _local) = setup_two_file_plan();
    let receipt = ExecutionEngine.apply(&plan_id, &store).unwrap();
    std::fs::write(&receipt.backup_paths[0], br#"{"model":"tampered"}"#).unwrap();

    let error = apply_rollback(&receipt.id).unwrap_err();

    assert_eq!(error.code, AgentErrorCode::ResourceChanged);
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&std::fs::read(shared).unwrap()).unwrap()
            ["model"],
        "new"
    );
}

#[test]
#[serial_test::serial(home_env)]
fn unowned_project_plugin_directory_cannot_be_replaced() {
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
    std::fs::write(target.join("version.txt"), "user-owned").unwrap();
    std::fs::write(source.join("version.txt"), "replacement").unwrap();
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
    let plan_id = PlanId::from("unowned-plugin-directory");
    let plan = MutationPlan {
        id: plan_id.clone(),
        agent_id: AgentId::from("codex"),
        context,
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

    let error = ExecutionEngine.apply(&plan_id, &store).unwrap_err();

    assert_eq!(error.code, AgentErrorCode::PermissionDenied);
    assert_eq!(
        std::fs::read_to_string(target.join("version.txt")).unwrap(),
        "user-owned"
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
    seed_existing_directory_ownership(&plan, &registry);
    store.insert(plan).unwrap();
    let receipt = ExecutionEngine.apply(&plan_id, &store).unwrap();
    std::fs::write(
        std::path::Path::new(&receipt.backup_paths[0]).join("version.txt"),
        "tampered",
    )
    .unwrap();

    let error = apply_rollback(&receipt.id).unwrap_err();

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

    let rollback = apply_rollback(&receipt.id).unwrap();

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
fn post_publish_target_error_compensates_the_current_mutation() {
    let (_temp, store, plan_id, shared, local) = setup_two_file_plan();
    let receipt = ExecutionEngine
        .apply_with_faults(
            &plan_id,
            &store,
            &FailAt::new([ExecutionStep::ApplyPublished(0)]),
        )
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
fn published_receipt_error_keeps_the_committed_result() {
    let (temp, store, plan_id, shared, local) = setup_two_file_plan();
    let receipt = ExecutionEngine
        .apply_with_faults(
            &plan_id,
            &store,
            &FailAt::new([ExecutionStep::PersistReceiptPublished]),
        )
        .unwrap();

    assert_eq!(receipt.status, OperationStatus::Complete);
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
    assert!(temp
        .path()
        .join(".ad/history/operations")
        .join(format!("{}.json", receipt.id))
        .is_file());
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
fn execution_state_root_swap_cannot_redirect_transaction_artifacts() {
    let (temp, store, plan_id, _, _) = setup_two_file_plan();
    let root = temp.path().join(".ad");
    let moved = temp.path().join(".ad.original");
    let outside = temp.path().join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    let faults = SwapAncestorAt {
        step: ExecutionStep::Backup(0),
        paths: std::sync::Mutex::new(Some((root, moved.clone(), outside.clone()))),
    };

    let receipt = ExecutionEngine
        .apply_with_faults(&plan_id, &store, &faults)
        .unwrap();

    assert_eq!(receipt.status, OperationStatus::Complete);
    assert!(moved
        .join("backups/operations")
        .join(receipt.id.as_str())
        .join("manifest.json")
        .exists());
    assert!(moved
        .join("history/operations")
        .join(format!("{}.json", receipt.id))
        .exists());
    assert_eq!(
        std::fs::read_dir(moved.join("state/operation-journals"))
            .unwrap()
            .count(),
        1
    );
    assert_eq!(std::fs::read_dir(outside).unwrap().count(), 0);
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
        source
    );
}

#[test]
#[serial_test::serial(home_env)]
fn project_skill_apply_and_inverse_rollback_reconcile_ownership() {
    let (_temp, registry, context, source) = setup_project_claude_skill();
    let receipt = apply_project_claude_skill(&registry, &context, &source);
    let resource = receipt.applied_resources[0].clone();
    let state = super::execution_state::ExecutionState::open().unwrap();
    let record = load_ownership_record(&state, &resource).unwrap().unwrap();

    assert_eq!(receipt.ownership_changes.len(), 1);
    assert_eq!(receipt.workspace_key, Some(record.workspace_key.clone()));
    assert_eq!(record.creating_receipt_id, receipt.id);
    assert_eq!(record.updated_by_receipt_id, receipt.id);
    assert_eq!(record.target_kind, ResourceStateKind::Symlink);

    let rollback = apply_rollback(&receipt.id).unwrap();

    assert_eq!(rollback.operation_kind, OperationKind::Rollback);
    assert_eq!(rollback.parent_receipt_id.as_ref(), Some(&receipt.id));
    assert_eq!(rollback.ownership_changes.len(), 1);
    assert_eq!(
        rollback.ownership_changes[0].kind,
        ResourceOwnershipChangeKind::Remove
    );
    assert!(load_ownership_record(&state, &resource).unwrap().is_none());
    assert!(
        !std::path::Path::new(context.project_path.as_deref().unwrap())
            .join(".claude/skills/demo")
            .exists()
    );
}

#[test]
#[serial_test::serial(home_env)]
fn rollback_preview_rejects_a_different_workspace_context() {
    let (_temp, registry, context, source) = setup_project_claude_skill();
    let receipt = apply_project_claude_skill(&registry, &context, &source);
    let plans = PlanStore::default();
    let wrong_context = AgentContext {
        installation_id: context.installation_id.clone(),
        project_path: None,
    };

    let error = ExecutionEngine
        .preview_rollback_bound(&receipt.id, &wrong_context, &plans)
        .unwrap_err();

    assert_eq!(error.code, AgentErrorCode::ResourceChanged);
    assert!(
        std::path::Path::new(context.project_path.as_deref().unwrap())
            .join(".claude/skills/demo")
            .is_symlink()
    );
}

#[test]
#[serial_test::serial(home_env)]
fn pre_ownership_v2_receipt_cannot_rollback_a_project_skill() {
    let (temp, registry, context, source) = setup_project_claude_skill();
    let receipt = apply_project_claude_skill(&registry, &context, &source);
    let resource = receipt.applied_resources[0].clone();
    let state = super::execution_state::ExecutionState::open().unwrap();
    let record = load_ownership_record(&state, &resource).unwrap().unwrap();
    apply_ownership_changes(
        &state,
        &[ResourceOwnershipChange {
            kind: ResourceOwnershipChangeKind::Remove,
            record_id: record.id.clone(),
            previous_record: Some(record),
            record: None,
        }],
    )
    .unwrap();
    let receipt_path = temp
        .path()
        .join(".ad/history/operations")
        .join(format!("{}.json", receipt.id));
    let mut value =
        serde_json::from_slice::<serde_json::Value>(&std::fs::read(&receipt_path).unwrap())
            .unwrap();
    value.as_object_mut().unwrap().remove("ownershipChanges");
    value
        .as_object_mut()
        .unwrap()
        .remove("ownershipEvidenceVersion");
    std::fs::write(&receipt_path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();

    let error = ExecutionEngine
        .preview_rollback(&receipt.id, &PlanStore::default())
        .unwrap_err();

    assert_eq!(error.code, AgentErrorCode::Unsupported);
    assert!(std::fs::symlink_metadata(
        std::path::Path::new(context.project_path.as_deref().unwrap()).join(".claude/skills/demo")
    )
    .unwrap()
    .file_type()
    .is_symlink());
}

#[test]
#[serial_test::serial(home_env)]
fn unowned_project_skill_link_cannot_be_removed() {
    let (_temp, registry, context, source) = setup_project_claude_skill();
    let target =
        std::path::Path::new(context.project_path.as_deref().unwrap()).join(".claude/skills/demo");
    std::os::unix::fs::symlink(std::fs::canonicalize(&source).unwrap(), &target).unwrap();
    let resource = ResourceRef {
        installation_id: context.installation_id.clone(),
        project_path: context.project_path.clone(),
        kind: ResourceKind::Skills,
        scope: ResourceScope::Project,
        logical_id: "demo".into(),
    };
    let plan = registry
        .adapter("claude-code")
        .unwrap()
        .skills()
        .unwrap()
        .plan_set_enabled(&context, &resource, false)
        .unwrap();
    let plan_id = plan.id.clone();
    let plans = PlanStore::default();
    plans.insert(plan).unwrap();

    let error = ExecutionEngine.apply(&plan_id, &plans).unwrap_err();

    assert_eq!(error.code, AgentErrorCode::PermissionDenied);
    assert!(std::fs::symlink_metadata(&target)
        .unwrap()
        .file_type()
        .is_symlink());
}

#[test]
#[serial_test::serial(home_env)]
fn project_skill_rollback_refuses_changed_owned_artifact() {
    let (_temp, registry, context, source) = setup_project_claude_skill();
    let receipt = apply_project_claude_skill(&registry, &context, &source);
    std::fs::write(source.join("SKILL.md"), "---\nname: demo\n---\nchanged\n").unwrap();

    let error = ExecutionEngine
        .preview_rollback(&receipt.id, &PlanStore::default())
        .unwrap_err();

    assert_eq!(error.code, AgentErrorCode::ResourceChanged);
    assert!(std::fs::symlink_metadata(
        std::path::Path::new(context.project_path.as_deref().unwrap()).join(".claude/skills/demo")
    )
    .unwrap()
    .file_type()
    .is_symlink());
}

#[test]
#[serial_test::serial(home_env)]
fn execution_skill_symlink_ancestor_swap_cannot_modify_outside_sentinel() {
    let temp = tempfile::tempdir().unwrap();
    std::env::set_var("AD_HOME", temp.path());
    std::env::remove_var("CODEX_HOME");
    let skills = temp.path().join(".claude/skills");
    let moved = temp.path().join(".claude/skills.original");
    let outside = temp.path().join("outside");
    std::fs::create_dir_all(&skills).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    std::os::unix::fs::symlink("outside-original", outside.join("demo")).unwrap();
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
    let faults = SwapAncestorAt {
        step: ExecutionStep::Apply(0),
        paths: std::sync::Mutex::new(Some((skills, moved.clone(), outside.clone()))),
    };

    let receipt = ExecutionEngine
        .apply_with_faults(&plan_id, &store, &faults)
        .unwrap();

    assert_eq!(receipt.status, OperationStatus::Complete);
    assert_eq!(std::fs::read_link(moved.join("demo")).unwrap(), source);
    assert_eq!(
        std::fs::read_link(outside.join("demo")).unwrap(),
        std::path::PathBuf::from("outside-original")
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
    seed_existing_directory_ownership(&plan, &registry);
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
