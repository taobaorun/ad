use chrono::Utc;
use serial_test::serial;

use super::*;
use crate::agents::{load_skill_catalog_snapshot, SkillSourceRequest};
use crate::models::{SkillSource, SkillSourceType};

fn git_source() -> SkillSource {
    SkillSource {
        id: format!("skill-source:{}", uuid::Uuid::new_v4()),
        source_type: SkillSourceType::Git,
        url: "https://example.com/team/skills.git".into(),
        branch: None,
        subdirectory: None,
        auto_update: false,
        added_at: Utc::now(),
    }
}

fn staged_git_checkout(
    source: &SkillSource,
    body: &str,
    revision: &str,
) -> super::super::StagedGitSkillSourceBinding {
    let operation = crate::fs::paths::skill_acquisition_staging_dir()
        .unwrap()
        .join(uuid::Uuid::new_v4().to_string());
    std::fs::create_dir_all(operation.join("source/review")).unwrap();
    std::fs::write(
        operation.join("source/review/SKILL.md"),
        format!("---\nname: review\n---\n{body}"),
    )
    .unwrap();
    super::super::skill_source_bindings::stage_existing_git_checkout_for_test(
        source, operation, revision,
    )
    .unwrap()
}

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
        binding: None,
        previous_binding: None,
        rollback_of: None,
        affected_resources: Vec::new(),
        affected_workspaces: Vec::new(),
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
fn recovery_reprojects_resources_after_a_durable_git_catalog_commit() {
    let home = tempfile::tempdir().unwrap();
    std::env::set_var("AD_HOME", home.path());
    let source = git_source();
    let first_staged = staged_git_checkout(&source, "first", &"a".repeat(40));
    let (first, first_publication) =
        publish_staged_git_skill_source_binding(first_staged, None).unwrap();
    first_publication.commit();
    let request = SkillSourceRequest {
        display_name: "Team skills".into(),
        source_type: SkillSourceType::Git,
        location: source.url.clone(),
        branch: None,
        subdirectory: None,
        auto_update: false,
    };
    let mut document = super::super::skill_catalog::SkillCatalogDocument::empty();
    document
        .add_binding(source.id.clone(), &request, first.clone(), Utc::now())
        .unwrap();
    let before_bytes = document.render().unwrap();
    let before_catalog_revision = ContentDigest::sha256(&before_bytes);
    let state = ExecutionState::open().unwrap();
    state
        .state()
        .write_atomic("skill_catalog.json", &before_bytes)
        .unwrap();
    persist_resource_catalog_projection(
        state.state(),
        &load_skill_catalog_state_from(state.state())
            .unwrap()
            .snapshot(),
    )
    .unwrap();

    let second_staged = staged_git_checkout(&source, "second", &"b".repeat(40));
    let (second, second_publication) =
        publish_staged_git_skill_source_binding(second_staged, Some(&first)).unwrap();
    second_publication.commit();
    document
        .update_binding(&source.id, second.clone(), Utc::now())
        .unwrap();
    let after_bytes = document.render().unwrap();
    let after_catalog_revision = ContentDigest::sha256(&after_bytes);
    state
        .state()
        .write_atomic("skill_catalog.json", &after_bytes)
        .unwrap();
    let stale = crate::agents::load_resource_catalog_snapshot().unwrap();
    assert_eq!(
        stale.sources[&source.id]
            .binding
            .as_ref()
            .unwrap()
            .physical_root,
        first.physical_root
    );
    let recovery_receipt = SkillCatalogReceipt {
        schema_version: RECEIPT_SCHEMA_VERSION,
        id: ReceiptId::from(format!("skill-catalog-receipt:{}", uuid::Uuid::new_v4())),
        plan_id: PlanId::from(format!("skill-catalog-plan:{}", uuid::Uuid::new_v4())),
        action: SkillCatalogAction::Update,
        source_id: source.id.clone(),
        before_catalog_revision,
        after_catalog_revision,
        artifact: None,
        binding: Some(second.clone()),
        previous_binding: Some(first),
        rollback_of: None,
        affected_resources: Vec::new(),
        affected_workspaces: Vec::new(),
        backup_id: None,
        status: SkillCatalogReceiptStatus::Complete,
        created_at: Utc::now(),
    };
    write_applying_journal(&state, recovery_receipt);

    let report = recover_skill_catalog_state().unwrap();

    assert_eq!(report.recovered, 1);
    let recovered = crate::agents::load_resource_catalog_snapshot().unwrap();
    assert_eq!(
        recovered.sources[&source.id]
            .binding
            .as_ref()
            .unwrap()
            .physical_root,
        second.physical_root
    );
    let resource_id = catalog_resource_id(&source.id, ResourceKind::Skills, "review");
    let resolved = crate::agents::resolve_catalog_resource(&resource_id).unwrap();
    assert!(resolved
        .physical_path
        .starts_with(std::fs::canonicalize(&second.physical_root).unwrap()));
    assert!(
        std::fs::read_to_string(resolved.physical_path.join("SKILL.md"))
            .unwrap()
            .contains("second")
    );
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

#[test]
#[serial(home_env)]
fn receipt_rollback_switches_catalog_and_shared_git_current() {
    let home = tempfile::tempdir().unwrap();
    std::env::set_var("AD_HOME", home.path());
    let source = git_source();
    let first_staged = staged_git_checkout(&source, "first", &"a".repeat(40));
    let (first, first_publication) =
        publish_staged_git_skill_source_binding(first_staged, None).unwrap();
    first_publication.commit();
    let second_staged = staged_git_checkout(&source, "second", &"b".repeat(40));
    let (second, second_publication) =
        publish_staged_git_skill_source_binding(second_staged, Some(&first)).unwrap();
    second_publication.commit();

    let request = SkillSourceRequest {
        display_name: "Team skills".into(),
        source_type: SkillSourceType::Git,
        location: source.url.clone(),
        branch: None,
        subdirectory: None,
        auto_update: false,
    };
    let mut document = super::super::skill_catalog::SkillCatalogDocument::empty();
    document
        .add_binding(source.id.clone(), &request, first.clone(), Utc::now())
        .unwrap();
    let before_catalog_revision = document.revision().unwrap();
    document
        .update_binding(&source.id, second.clone(), Utc::now())
        .unwrap();
    let after_bytes = document.render().unwrap();
    let after_catalog_revision = ContentDigest::sha256(&after_bytes);
    let state = ExecutionState::open().unwrap();
    state
        .state()
        .write_atomic("skill_catalog.json", &after_bytes)
        .unwrap();
    let original_receipt = SkillCatalogReceipt {
        schema_version: RECEIPT_SCHEMA_VERSION,
        id: ReceiptId::from(format!("skill-catalog-receipt:{}", uuid::Uuid::new_v4())),
        plan_id: PlanId::from(format!("skill-catalog-plan:{}", uuid::Uuid::new_v4())),
        action: SkillCatalogAction::Update,
        source_id: source.id.clone(),
        before_catalog_revision,
        after_catalog_revision,
        artifact: None,
        binding: Some(second.clone()),
        previous_binding: Some(first.clone()),
        rollback_of: None,
        affected_resources: Vec::new(),
        affected_workspaces: Vec::new(),
        backup_id: None,
        status: SkillCatalogReceiptStatus::Complete,
        created_at: Utc::now(),
    };
    persist_receipt(&state, &original_receipt).unwrap();
    let plans = SkillCatalogPlanStore::default();

    let preview = preview_rollback_skill_catalog_source(&original_receipt.id, &plans).unwrap();
    assert_eq!(preview.rollback_of.as_ref(), Some(&original_receipt.id));
    assert_eq!(preview.binding.as_ref(), Some(&first));
    assert_eq!(preview.current_binding.as_ref(), Some(&second));
    let report = apply_skill_catalog_plan(
        &plans,
        &SkillCatalogPlanClaim {
            plan_id: preview.id,
            risk_fingerprint: preview.risk_fingerprint,
            confirmed: true,
        },
    )
    .unwrap();

    assert_eq!(report.outcome, SkillCatalogOperationOutcome::Changed);
    let rollback_receipt = report.receipt.unwrap();
    assert_eq!(
        rollback_receipt.rollback_of.as_ref(),
        Some(&original_receipt.id)
    );
    assert_eq!(rollback_receipt.binding.as_ref(), Some(&first));
    assert_eq!(rollback_receipt.previous_binding.as_ref(), Some(&second));
    assert!(std::fs::read_to_string(
        std::path::Path::new(&first.stable_root).join("review/SKILL.md")
    )
    .unwrap()
    .contains("first"));
    assert_eq!(
        load_skill_catalog_snapshot().unwrap().entries[0]
            .current_binding
            .as_ref(),
        Some(&first)
    );
}
