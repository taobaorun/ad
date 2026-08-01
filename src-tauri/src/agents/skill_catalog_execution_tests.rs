use chrono::Utc;
use serial_test::serial;

use super::*;

fn receipt(before: ContentDigest, after: ContentDigest) -> SkillCatalogReceipt {
    SkillCatalogReceipt {
        schema_version: RECEIPT_SCHEMA_VERSION,
        id: ReceiptId::from(format!("skill-catalog-receipt:{}", uuid::Uuid::new_v4())),
        plan_id: PlanId::from(format!("skill-catalog-plan:{}", uuid::Uuid::new_v4())),
        action: SkillCatalogAction::Remove,
        source_id: format!("skill-source:{}", uuid::Uuid::new_v4()),
        before_catalog_revision: before,
        after_catalog_revision: after,
        artifact: None,
        backup_id: None,
        status: SkillCatalogReceiptStatus::Complete,
        created_at: Utc::now(),
    }
}

fn write_applying_journal(state: &ExecutionState, receipt: SkillCatalogReceipt) -> String {
    let journal = CatalogJournal {
        schema_version: JOURNAL_SCHEMA_VERSION,
        instance_id: "crashed-instance".into(),
        operation_id: uuid::Uuid::new_v4().to_string(),
        state: CatalogJournalState::Applying,
        receipt,
    };
    let name = journal_name(&journal.receipt.plan_id);
    persist_journal(state, &name, &journal, false).unwrap();
    name
}

#[test]
#[serial(home_env)]
fn recovery_finishes_receipt_when_catalog_commit_is_durable() {
    let home = tempfile::tempdir().unwrap();
    std::env::set_var("AD_HOME", home.path());
    let state = ExecutionState::open().unwrap();
    let before = load_skill_catalog_state_from(state.state())
        .unwrap()
        .revision;
    let committed = br#"{"schemaVersion":1,"entries":[]}"#;
    let after = ContentDigest::sha256(committed);
    state
        .state()
        .write_atomic("skill_catalog.json", committed)
        .unwrap();
    let name = write_applying_journal(&state, receipt(before, after));

    let report = recover_skill_catalog_state().unwrap();

    assert_eq!(report.recovered, 1);
    let journal: CatalogJournal =
        serde_json::from_slice(&state.skill_catalog_journals().read(&name).unwrap()).unwrap();
    assert_eq!(journal.state, CatalogJournalState::Committed);
    assert_eq!(journal.receipt.status, SkillCatalogReceiptStatus::Recovered);
}

#[test]
#[serial(home_env)]
fn recovery_compensates_when_catalog_commit_never_happened() {
    let home = tempfile::tempdir().unwrap();
    std::env::set_var("AD_HOME", home.path());
    let state = ExecutionState::open().unwrap();
    let before = load_skill_catalog_state_from(state.state())
        .unwrap()
        .revision;
    let name = write_applying_journal(
        &state,
        receipt(before, ContentDigest::from("sha256:future")),
    );

    let report = recover_skill_catalog_state().unwrap();

    assert_eq!(report.compensated, 1);
    let journal: CatalogJournal =
        serde_json::from_slice(&state.skill_catalog_journals().read(&name).unwrap()).unwrap();
    assert_eq!(journal.state, CatalogJournalState::Compensated);
    assert_eq!(
        journal.receipt.status,
        SkillCatalogReceiptStatus::Compensated
    );
}

#[test]
#[serial(home_env)]
fn corrupt_recovery_input_blocks_catalog_mutation() {
    let home = tempfile::tempdir().unwrap();
    std::env::set_var("AD_HOME", home.path());
    let state = ExecutionState::open().unwrap();
    state
        .skill_catalog_journals()
        .write_atomic_new("corrupt.json", b"not-json")
        .unwrap();

    let report = recover_skill_catalog_state().unwrap();

    assert_eq!(report.repair_required, 1);
    assert!(!report.writable());
    assert!(matches!(
        ensure_recovery_writable(&state),
        Err(SkillCatalogExecutionError::RepairRequired)
    ));
}
