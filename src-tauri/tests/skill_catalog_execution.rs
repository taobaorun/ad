use std::path::Path;

use ad_lib::agents::{
    apply_skill_catalog_plan, load_skill_catalog_snapshot, recover_skill_catalog_state,
    verify_skill_artifact, RiskFingerprint, SkillCatalogExecutionError,
    SkillCatalogOperationOutcome, SkillCatalogPlanClaim, SkillCatalogPlanStore, SkillSourceRequest,
    SkillSourceType,
};
use serial_test::serial;

fn write_skill(root: &Path, body: &str) {
    std::fs::create_dir_all(root.join("review/scripts")).unwrap();
    std::fs::write(
        root.join("review/SKILL.md"),
        format!("---\nname: review\n---\n{body}"),
    )
    .unwrap();
    std::fs::write(root.join("review/scripts/check.sh"), "#!/bin/sh\n").unwrap();
}

fn request(source: &Path) -> SkillSourceRequest {
    SkillSourceRequest {
        display_name: "Review tools".into(),
        source_type: SkillSourceType::Local,
        location: source.to_string_lossy().into_owned(),
        branch: None,
        subdirectory: None,
        auto_update: false,
    }
}

fn claim(view: &ad_lib::agents::SkillCatalogPlanView) -> SkillCatalogPlanClaim {
    SkillCatalogPlanClaim {
        plan_id: view.id.clone(),
        risk_fingerprint: view.risk_fingerprint.clone(),
        confirmed: true,
    }
}

#[test]
#[serial(home_env)]
fn preview_is_non_publishing_and_source_drift_fails_closed() {
    let home = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    std::env::set_var("AD_HOME", home.path());
    write_skill(source.path(), "first");
    let store = SkillCatalogPlanStore::default();

    let plan = store.preview_add(request(source.path())).unwrap();

    assert!(!home.path().join(".ad/state/skill_catalog.json").exists());
    assert!(!home.path().join(".ad/artifacts/skills").exists());
    assert!(plan.source_id.starts_with("skill-source:"));
    assert!(plan.artifact.is_some());
    let unconfirmed = SkillCatalogPlanClaim {
        confirmed: false,
        ..claim(&plan)
    };
    assert!(apply_skill_catalog_plan(&store, &unconfirmed).is_err());
    let wrong_risk = SkillCatalogPlanClaim {
        risk_fingerprint: RiskFingerprint::from("risk:changed"),
        ..claim(&plan)
    };
    assert!(apply_skill_catalog_plan(&store, &wrong_risk).is_err());
    write_skill(source.path(), "changed after preview");

    let error = apply_skill_catalog_plan(&store, &claim(&plan)).unwrap_err();

    assert!(matches!(error, SkillCatalogExecutionError::SourceChanged));
    assert!(!home.path().join(".ad/state/skill_catalog.json").exists());
    write_skill(source.path(), "first");
    let report = apply_skill_catalog_plan(&store, &claim(&plan)).unwrap();
    assert_eq!(report.outcome, SkillCatalogOperationOutcome::Changed);
    assert!(report.receipt.is_some());
    assert!(apply_skill_catalog_plan(&store, &claim(&plan)).is_err());
}

#[test]
#[serial(home_env)]
fn catalog_refresh_keeps_old_artifact_pins_and_remove_deletes_no_content() {
    let home = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    std::env::set_var("AD_HOME", home.path());
    write_skill(source.path(), "first");
    let store = SkillCatalogPlanStore::default();
    let add = store.preview_add(request(source.path())).unwrap();
    apply_skill_catalog_plan(&store, &claim(&add)).unwrap();
    let first = load_skill_catalog_snapshot().unwrap().entries[0]
        .current_artifact
        .clone();
    let first_tree = verify_skill_artifact(&first).unwrap();
    let first_bytes = std::fs::read(first_tree.join("review/SKILL.md")).unwrap();
    let no_change = store.preview_update(&add.source_id).unwrap();
    let no_change_report = apply_skill_catalog_plan(&store, &claim(&no_change)).unwrap();
    assert_eq!(
        no_change_report.outcome,
        SkillCatalogOperationOutcome::NoChange
    );
    assert!(no_change_report.receipt.is_none());

    write_skill(source.path(), "second");
    let update = store.preview_update(&add.source_id).unwrap();
    apply_skill_catalog_plan(&store, &claim(&update)).unwrap();
    let second = load_skill_catalog_snapshot().unwrap().entries[0]
        .current_artifact
        .clone();

    assert_ne!(first.artifact_id, second.artifact_id);
    assert_eq!(
        std::fs::read(
            verify_skill_artifact(&first)
                .unwrap()
                .join("review/SKILL.md")
        )
        .unwrap(),
        first_bytes
    );
    assert!(verify_skill_artifact(&second).is_ok());

    let remove = store.preview_remove(&add.source_id).unwrap();
    apply_skill_catalog_plan(&store, &claim(&remove)).unwrap();

    assert!(load_skill_catalog_snapshot().unwrap().entries.is_empty());
    assert!(verify_skill_artifact(&first).is_ok());
    assert!(verify_skill_artifact(&second).is_ok());
    let recovery = recover_skill_catalog_state().unwrap();
    assert!(recovery.writable());
}

#[test]
#[serial(home_env)]
fn catalog_drift_after_preview_rejects_the_plan_without_publication() {
    let home = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    std::env::set_var("AD_HOME", home.path());
    write_skill(source.path(), "first");
    let store = SkillCatalogPlanStore::default();
    let plan = store.preview_add(request(source.path())).unwrap();
    std::fs::write(
        home.path().join(".ad/state/skill_catalog.json"),
        br#"{"schemaVersion":1,"entries":[]}"#,
    )
    .unwrap();

    let error = apply_skill_catalog_plan(&store, &claim(&plan)).unwrap_err();

    assert!(matches!(error, SkillCatalogExecutionError::CatalogChanged));
    assert!(!home.path().join(".ad/artifacts/skills").exists());
}
