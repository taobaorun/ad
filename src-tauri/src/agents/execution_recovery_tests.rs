use chrono::{Duration, Utc};

use super::execution_journal::{OperationJournal, OperationJournalHandle, OperationJournalState};
use super::execution_recovery::{recover_state, MutationRecoveryLease};
use super::execution_state::ExecutionState;
use super::*;

fn plan(id: &str) -> MutationPlan {
    let resource = ResourceRef {
        installation_id: InstallationId::from("codex:default"),
        project_path: Some("/Users/test/project".into()),
        kind: ResourceKind::Settings,
        scope: ResourceScope::Project,
        logical_id: "project-config".into(),
    };
    MutationPlan {
        id: PlanId::from(id),
        agent_id: AgentId::from("codex"),
        context: AgentContext {
            installation_id: resource.installation_id.clone(),
            project_path: resource.project_path.clone(),
        },
        read_set: Vec::new(),
        mutations: vec![PlannedMutation {
            resource,
            kind: MutationKind::Replace,
            expected_digest: None,
            media_type: "application/toml".into(),
            content: Some(serde_json::Value::String("model = 'new'".into())),
        }],
        expires_at: Utc::now() + Duration::minutes(5),
    }
}

fn only_journal(state: &ExecutionState) -> OperationJournal {
    let name = state
        .journals()
        .entry_names()
        .unwrap()
        .into_iter()
        .find(|name| name.to_string_lossy().ends_with(".json"))
        .unwrap();
    serde_json::from_slice(&state.journals().read(name.to_str().unwrap()).unwrap()).unwrap()
}

#[test]
fn startup_recovery_compensates_prepared_operations_and_cleans_backups() {
    let temp = tempfile::tempdir().unwrap();
    let state = ExecutionState::open_at(&temp.path().join(".ad")).unwrap();
    let receipt_id = ReceiptId::from("prepared-receipt");
    OperationJournalHandle::prepare(
        &plan("prepared-plan"),
        &receipt_id,
        "instance",
        "prepared-operation",
        &state,
    )
    .unwrap();
    state
        .backups()
        .create_directory(receipt_id.as_str())
        .unwrap();

    let report = recover_state(&state).unwrap();

    assert!(report.writable());
    assert_eq!(report.recovered, 1);
    assert_eq!(
        only_journal(&state).state,
        OperationJournalState::Compensated
    );
    assert!(state.backups().open_directory(receipt_id.as_str()).is_err());
    MutationRecoveryLease::acquire(&state).unwrap();
}

#[test]
fn startup_recovery_accepts_prepared_operations_before_backup_creation() {
    let temp = tempfile::tempdir().unwrap();
    let state = ExecutionState::open_at(&temp.path().join(".ad")).unwrap();
    OperationJournalHandle::prepare(
        &plan("prepared-without-backup-plan"),
        &ReceiptId::from("prepared-without-backup-receipt"),
        "instance",
        "prepared-without-backup-operation",
        &state,
    )
    .unwrap();

    let report = recover_state(&state).unwrap();

    assert!(report.writable());
    assert_eq!(report.recovered, 1);
    assert_eq!(
        only_journal(&state).state,
        OperationJournalState::Compensated
    );
}

#[test]
fn applying_without_a_receipt_becomes_repair_required_and_blocks_mutations() {
    let temp = tempfile::tempdir().unwrap();
    let state = ExecutionState::open_at(&temp.path().join(".ad")).unwrap();
    let mut journal = OperationJournalHandle::prepare(
        &plan("applying-plan"),
        &ReceiptId::from("applying-receipt"),
        "instance",
        "applying-operation",
        &state,
    )
    .unwrap();
    journal
        .transition(OperationJournalState::Applying, None)
        .unwrap();

    let report = recover_state(&state).unwrap();

    assert!(!report.writable());
    assert_eq!(report.repair_required, 1);
    assert_eq!(
        only_journal(&state).state,
        OperationJournalState::RepairRequired
    );
    let error = MutationRecoveryLease::acquire(&state).unwrap_err();
    assert_eq!(error.code, AgentErrorCode::PartialFailure);
}

#[test]
fn applying_with_a_complete_receipt_is_reconciled_as_committed() {
    let temp = tempfile::tempdir().unwrap();
    let state = ExecutionState::open_at(&temp.path().join(".ad")).unwrap();
    let plan = plan("complete-plan");
    let receipt_id = ReceiptId::from("complete-receipt");
    let mut journal = OperationJournalHandle::prepare(
        &plan,
        &receipt_id,
        "instance",
        "complete-operation",
        &state,
    )
    .unwrap();
    journal
        .transition(OperationJournalState::Applying, None)
        .unwrap();
    let receipt = OperationReceipt {
        schema_version: OPERATION_RECEIPT_SCHEMA_VERSION,
        id: receipt_id.clone(),
        plan_id: plan.id.clone(),
        operation_kind: OperationKind::Apply,
        parent_receipt_id: None,
        context: Some(plan.context),
        workspace_key: None,
        action_id: None,
        status: OperationStatus::Complete,
        applied_resources: Vec::new(),
        backup_paths: Vec::new(),
        post_apply_states: Vec::new(),
        manifest_digest: None,
        ownership_changes: Vec::new(),
        ownership_evidence_version: OWNERSHIP_EVIDENCE_VERSION,
        rollback: RollbackEligibility::available(),
        created_at: Some(Utc::now()),
        message: None,
    };
    state
        .history()
        .write_atomic_new(
            &format!("{receipt_id}.json"),
            &serde_json::to_vec(&receipt).unwrap(),
        )
        .unwrap();

    let report = recover_state(&state).unwrap();

    assert!(report.writable());
    assert_eq!(report.recovered, 1);
    assert_eq!(only_journal(&state).state, OperationJournalState::Committed);
}

#[test]
fn applying_receipt_replays_missing_ownership_before_commit() {
    let temp = tempfile::tempdir().unwrap();
    let state = ExecutionState::open_at(&temp.path().join(".ad")).unwrap();
    let plan = plan("ownership-recovery-plan");
    let receipt_id = ReceiptId::from("ownership-recovery-receipt");
    let mut journal = OperationJournalHandle::prepare(
        &plan,
        &receipt_id,
        "instance",
        "ownership-recovery-operation",
        &state,
    )
    .unwrap();
    journal
        .transition(OperationJournalState::Applying, None)
        .unwrap();
    let resource = ResourceRef {
        installation_id: plan.context.installation_id.clone(),
        project_path: plan.context.project_path.clone(),
        kind: ResourceKind::Plugins,
        scope: ResourceScope::Project,
        logical_id: "package:team:demo:1.0.0".into(),
    };
    let record_id = ownership_record_id(&resource);
    let digest = ContentDigest::sha256(b"owned-directory");
    let record = ResourceOwnershipRecord {
        schema_version: RESOURCE_OWNERSHIP_SCHEMA_VERSION,
        id: record_id.clone(),
        workspace_key: ownership_workspace_key(&resource).unwrap(),
        resource: resource.clone(),
        target_id: PhysicalTargetId::for_resource(&resource),
        target_path: temp
            .path()
            .join("project/plugins/demo")
            .to_string_lossy()
            .into_owned(),
        target_kind: ResourceStateKind::Directory,
        target_digest: digest.clone(),
        artifact_id: temp
            .path()
            .join("artifacts/demo")
            .to_string_lossy()
            .into_owned(),
        artifact_digest: digest.clone(),
        creating_receipt_id: receipt_id.clone(),
        updated_by_receipt_id: receipt_id.clone(),
    };
    let receipt = OperationReceipt {
        schema_version: OPERATION_RECEIPT_SCHEMA_VERSION,
        id: receipt_id.clone(),
        plan_id: plan.id,
        operation_kind: OperationKind::Apply,
        parent_receipt_id: None,
        context: Some(plan.context),
        workspace_key: None,
        action_id: None,
        status: OperationStatus::Complete,
        applied_resources: vec![resource.clone()],
        backup_paths: Vec::new(),
        post_apply_states: vec![AppliedResourceState {
            resource: resource.clone(),
            kind: ResourceStateKind::Directory,
            digest: Some(digest),
        }],
        manifest_digest: None,
        ownership_changes: vec![ResourceOwnershipChange {
            kind: ResourceOwnershipChangeKind::Upsert,
            record_id,
            previous_record: None,
            record: Some(record.clone()),
        }],
        ownership_evidence_version: OWNERSHIP_EVIDENCE_VERSION,
        rollback: RollbackEligibility::available(),
        created_at: Some(Utc::now()),
        message: None,
    };
    state
        .history()
        .write_atomic_new(
            &format!("{receipt_id}.json"),
            &serde_json::to_vec(&receipt).unwrap(),
        )
        .unwrap();

    let report = recover_state(&state).unwrap();

    assert!(report.writable());
    assert_eq!(report.recovered, 1);
    assert_eq!(only_journal(&state).state, OperationJournalState::Committed);
    assert_eq!(
        load_ownership_record(&state, &resource).unwrap(),
        Some(record)
    );
}

#[test]
fn corrupt_or_future_journals_block_only_mutations_not_recovery_inspection() {
    let temp = tempfile::tempdir().unwrap();
    let state = ExecutionState::open_at(&temp.path().join(".ad")).unwrap();
    state
        .journals()
        .write_atomic_new("corrupt.json", b"not-json")
        .unwrap();
    state
        .journals()
        .write_atomic_new(
            "future.json",
            br#"{"schemaVersion":999,"state":"prepared"}"#,
        )
        .unwrap();

    let report = recover_state(&state).unwrap();

    assert!(!report.writable());
    assert_eq!(report.inspected, 2);
    assert_eq!(report.diagnostics.len(), 2);
    let error = MutationRecoveryLease::acquire(&state).unwrap_err();
    assert_eq!(error.code, AgentErrorCode::PartialFailure);
}

#[test]
fn legacy_v1_journals_are_upgraded_when_recovery_transitions_them() {
    let temp = tempfile::tempdir().unwrap();
    let state = ExecutionState::open_at(&temp.path().join(".ad")).unwrap();
    let plan = plan("legacy-plan");
    let receipt_id = ReceiptId::from("legacy-receipt");
    let journal = OperationJournal {
        schema_version: 1,
        instance_id: "legacy-instance".into(),
        operation_id: "legacy-operation".into(),
        plan_id: plan.id,
        planned_receipt_id: receipt_id,
        state: OperationJournalState::Prepared,
        targets: Vec::new(),
        receipt_id: None,
    };
    state
        .journals()
        .write_atomic_new("legacy.json", &serde_json::to_vec(&journal).unwrap())
        .unwrap();

    let report = recover_state(&state).unwrap();
    let recovered = only_journal(&state);

    assert!(report.writable());
    assert_eq!(recovered.schema_version, 2);
    assert_eq!(recovered.state, OperationJournalState::Compensated);
}
