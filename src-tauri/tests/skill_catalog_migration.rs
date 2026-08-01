use std::path::{Path, PathBuf};

use ad_lib::agents::{
    apply_legacy_project_skill_migration, apply_project_collection_action_plan,
    apply_skill_catalog_plan, builtin_registry, inspect_legacy_skill_inventory,
    inspect_project_workspace_inventory, preview_legacy_project_skill_migration,
    preview_project_collection_action, recover_legacy_skill_migrations,
    rollback_legacy_project_skill_migration, LegacySkillHealth, LegacySkillLinkTargetKind,
    LegacySkillMigrationOutcome, LegacySkillMigrationPlanClaim, LegacySkillMigrationPlanStore,
    LegacySkillMigrationStatus, PlanStore, ProjectCollectionActionRequest, ResourceAction,
    SkillCatalogPlanClaim, SkillCatalogPlanStore, SkillSourceRequest, SkillSourceType,
};
use serial_test::serial;

fn write_legacy_source_registry(home: &Path, sources: serde_json::Value) {
    let path = home.join(".ad/state/skill_sources.json");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, serde_json::to_vec_pretty(&sources).unwrap()).unwrap();
}

fn write_legacy_project_state(home: &Path, file: &str, config: &serde_json::Value) {
    let path = home.join(".ad/state/project_skills").join(file);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, serde_json::to_vec_pretty(config).unwrap()).unwrap();
}

fn legacy_state_name(project: &Path) -> String {
    let lower = project
        .to_string_lossy()
        .trim_start_matches('/')
        .to_lowercase();
    let mut slug = String::with_capacity(lower.len());
    let mut last_dash = false;
    for character in lower.chars() {
        let character = if matches!(character, '/' | '_' | ' ') {
            '-'
        } else {
            character
        };
        if character == '-' {
            if !last_dash {
                slug.push('-');
            }
            last_dash = true;
        } else {
            slug.push(character);
            last_dash = false;
        }
    }
    format!("{}.json", slug.trim_end_matches('-'))
}

fn catalog_claim(plan: &ad_lib::agents::SkillCatalogPlanView) -> SkillCatalogPlanClaim {
    SkillCatalogPlanClaim {
        plan_id: plan.id.clone(),
        risk_fingerprint: plan.risk_fingerprint.clone(),
        confirmed: true,
    }
}

fn migration_claim(
    plan: &ad_lib::agents::LegacySkillMigrationPlanView,
) -> LegacySkillMigrationPlanClaim {
    LegacySkillMigrationPlanClaim {
        plan_id: plan.id.clone(),
        risk_fingerprint: plan.risk_fingerprint.clone(),
        confirmed: true,
    }
}

fn install_managed_project_skill(home: &Path, project: &Path, source: &Path) {
    std::fs::create_dir_all(source.join("review")).unwrap();
    std::fs::write(source.join("review/SKILL.md"), "# Review\n").unwrap();
    let catalogs = SkillCatalogPlanStore::default();
    let add = catalogs
        .preview_add(SkillSourceRequest {
            display_name: "Managed review".into(),
            source_type: SkillSourceType::Local,
            location: source.to_string_lossy().into_owned(),
            branch: None,
            subdirectory: None,
            auto_update: false,
        })
        .unwrap();
    apply_skill_catalog_plan(&catalogs, &catalog_claim(&add)).unwrap();

    install_available_project_skill(project, "review");
    assert!(home.join(".ad/state/resource-ownership").is_dir());
}

fn install_available_project_skill(project: &Path, logical_id: &str) {
    let installation = builtin_registry()
        .discover()
        .into_iter()
        .find(|installation| installation.agent_id.as_str() == "claude-code")
        .unwrap();
    let inventory = inspect_project_workspace_inventory(&installation.id, project).unwrap();
    let skill = inventory
        .skills
        .resources
        .iter()
        .find(|resource| {
            resource.logical_id == logical_id
                && resource
                    .management
                    .actions
                    .iter()
                    .any(|action| action.action == ResourceAction::Install)
        })
        .unwrap();
    let plans = PlanStore::default();
    let preview = preview_project_collection_action(
        &installation.id,
        project,
        ProjectCollectionActionRequest {
            workspace_key: inventory.workspace.key.clone(),
            inventory_revision: inventory.revision,
            resource_key: skill.key.clone(),
            action: ResourceAction::Install,
        },
        &plans,
    )
    .unwrap();
    apply_project_collection_action_plan(
        &preview.plan.id,
        &preview.plan.context,
        &preview.plan.risk_fingerprint,
        true,
        &plans,
    )
    .unwrap();
}

fn ready_legacy_project(home: &Path, name: &str) -> (PathBuf, String) {
    std::fs::create_dir_all(home.join(".claude")).unwrap();
    let project = home.join(name);
    std::fs::create_dir_all(&project).unwrap();
    let project = std::fs::canonicalize(project).unwrap();
    let source = home.join(format!("{name}-source"));
    install_managed_project_skill(home, &project, &source);
    write_legacy_source_registry(
        home,
        serde_json::json!([{
            "id": "legacy",
            "sourceType": "local",
            "url": source,
            "autoUpdate": false,
            "addedAt": "2026-08-01T08:00:00Z"
        }]),
    );
    let state_name = legacy_state_name(&project);
    write_legacy_project_state(
        home,
        &state_name,
        &serde_json::json!({
            "projectPath": project,
            "listedSkills": ["legacy/review"],
            "mode": "allowlist"
        }),
    );
    (project, state_name)
}

#[test]
#[serial(home_env)]
fn legacy_inventory_is_read_only_idempotent_and_blocks_ambiguous_ownership() {
    let home = tempfile::tempdir().unwrap();
    std::env::set_var("AD_HOME", home.path());
    let checkout = home.path().join(".ad/skill-library/shared/review");
    std::fs::create_dir_all(&checkout).unwrap();
    std::fs::write(checkout.join("SKILL.md"), "# Review\n").unwrap();
    let external = home.path().join("external-skill");
    std::fs::create_dir_all(&external).unwrap();
    std::fs::write(external.join("SKILL.md"), "# External\n").unwrap();
    let sentinel = home.path().join("outside-sentinel");
    std::fs::write(&sentinel, "unchanged").unwrap();
    write_legacy_source_registry(
        home.path(),
        serde_json::json!([
            {
                "id": "shared",
                "sourceType": "git",
                "url": "https://example.com/shared.git",
                "autoUpdate": false,
                "addedAt": "2026-08-01T08:00:00Z"
            },
            {
                "id": "../outside",
                "sourceType": "git",
                "url": "https://token@example.com/private.git",
                "autoUpdate": false,
                "addedAt": "2026-08-01T08:00:00Z"
            },
            {
                "id": "missing",
                "sourceType": "local",
                "url": home.path().join("missing").to_string_lossy(),
                "autoUpdate": false,
                "addedAt": "2026-08-01T08:00:00Z"
            }
        ]),
    );
    let project = home.path().join("project");
    std::fs::create_dir_all(project.join(".claude/skills")).unwrap();
    std::os::unix::fs::symlink(&checkout, project.join(".claude/skills/review")).unwrap();
    std::os::unix::fs::symlink(&external, project.join(".claude/skills/external")).unwrap();
    let config = serde_json::json!({
        "projectPath": project.to_string_lossy(),
        "listedSkills": ["shared/review", "unknown/missing"],
        "mode": "allowlist"
    });
    write_legacy_project_state(home.path(), "first.json", &config);
    write_legacy_project_state(home.path(), "alias.json", &config);
    let registry_before = std::fs::read(home.path().join(".ad/state/skill_sources.json")).unwrap();
    let first = inspect_legacy_skill_inventory().unwrap();
    let second = inspect_legacy_skill_inventory().unwrap();

    assert_eq!(first, second);
    assert_eq!(
        std::fs::read(home.path().join(".ad/state/skill_sources.json")).unwrap(),
        registry_before
    );
    assert_eq!(std::fs::read_to_string(&sentinel).unwrap(), "unchanged");
    let unsafe_source = first
        .sources
        .iter()
        .find(|source| source.legacy_id == "../outside")
        .unwrap();
    assert_eq!(unsafe_source.health, LegacySkillHealth::Blocked);
    assert!(!unsafe_source.display_location.contains("token"));
    assert!(first.projects.iter().all(|project| {
        project.health == LegacySkillHealth::Blocked && project.missing_source_ids == ["unknown"]
    }));
    let links = &first.projects[0].links;
    assert!(
        links.iter().any(|link| {
            link.logical_id == "review"
                && link.target_kind == LegacySkillLinkTargetKind::LegacyCheckout
        }),
        "unexpected legacy links: {links:?}"
    );
    assert!(links.iter().any(|link| {
        link.logical_id == "external"
            && link.target_kind == LegacySkillLinkTargetKind::External
            && link.health == LegacySkillHealth::Blocked
    }));
}

#[test]
#[serial(home_env)]
fn migrated_project_state_archives_only_after_preview_and_can_be_restored() {
    let home = tempfile::tempdir().unwrap();
    std::env::set_var("AD_HOME", home.path());
    std::fs::create_dir_all(home.path().join(".claude")).unwrap();
    let project = home.path().join("project");
    std::fs::create_dir_all(&project).unwrap();
    let project = std::fs::canonicalize(project).unwrap();
    let source = home.path().join("source");
    install_managed_project_skill(home.path(), &project, &source);
    write_legacy_source_registry(
        home.path(),
        serde_json::json!([{
            "id": "legacy",
            "sourceType": "local",
            "url": source,
            "autoUpdate": false,
            "addedAt": "2026-08-01T08:00:00Z"
        }]),
    );
    let state_name = legacy_state_name(&project);
    let legacy = serde_json::json!({
        "projectPath": project,
        "listedSkills": ["legacy/review"],
        "mode": "allowlist"
    });
    write_legacy_project_state(home.path(), &state_name, &legacy);
    let source_state = home
        .path()
        .join(".ad/state/project_skills")
        .join(&state_name);
    let original_bytes = std::fs::read(&source_state).unwrap();

    let inventory = inspect_legacy_skill_inventory().unwrap();
    assert_eq!(inventory.projects.len(), 1);
    assert_eq!(
        inventory.projects[0].migration_status,
        LegacySkillMigrationStatus::ReadyToArchive
    );
    assert!(inventory.projects[0]
        .links
        .iter()
        .any(|link| link.target_kind == LegacySkillLinkTargetKind::ManagedArtifact));

    let plans = LegacySkillMigrationPlanStore::default();
    let preview = preview_legacy_project_skill_migration(&project, &plans).unwrap();
    assert!(source_state.exists(), "preview must not move legacy state");
    let report = apply_legacy_project_skill_migration(&plans, &migration_claim(&preview)).unwrap();
    assert_eq!(report.outcome, LegacySkillMigrationOutcome::Archived);
    let receipt = report.receipt.unwrap();
    assert!(!source_state.exists());
    let archived_state = std::fs::read_dir(home.path().join(".ad/archive/skill-catalog"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.to_string_lossy().ends_with(".legacy.json"))
        .unwrap();
    assert_eq!(std::fs::read(archived_state).unwrap(), original_bytes);
    assert!(project.join(".claude/skills/review").is_symlink());

    let after = inspect_legacy_skill_inventory().unwrap();
    assert!(after.projects.is_empty());
    assert_eq!(after.archives.len(), 1);
    assert_eq!(after.archives[0].receipt_id, receipt.id);
    assert!(preview_legacy_project_skill_migration(&project, &plans).is_err());
    assert!(recover_legacy_skill_migrations().unwrap().writable());

    let rollback = rollback_legacy_project_skill_migration(&receipt.id).unwrap();
    assert_eq!(rollback.outcome, LegacySkillMigrationOutcome::Restored);
    assert_eq!(std::fs::read(&source_state).unwrap(), original_bytes);
    assert!(project.join(".claude/skills/review").is_symlink());

    let second_preview = preview_legacy_project_skill_migration(&project, &plans).unwrap();
    let second =
        apply_legacy_project_skill_migration(&plans, &migration_claim(&second_preview)).unwrap();
    assert_eq!(second.outcome, LegacySkillMigrationOutcome::Archived);
    let second_receipt = second.receipt.unwrap();
    assert_ne!(second_receipt.id, receipt.id);
    assert!(rollback_legacy_project_skill_migration(&receipt.id).is_err());
    let second_rollback = rollback_legacy_project_skill_migration(&second_receipt.id).unwrap();
    assert_eq!(
        second_rollback.outcome,
        LegacySkillMigrationOutcome::Restored
    );
    assert_eq!(std::fs::read(&source_state).unwrap(), original_bytes);
}

#[test]
#[serial(home_env)]
fn migration_rejects_a_stale_preview_without_archiving_changed_bytes() {
    let home = tempfile::tempdir().unwrap();
    std::env::set_var("AD_HOME", home.path());
    let (project, state_name) = ready_legacy_project(home.path(), "stale-preview");
    let state_path = home
        .path()
        .join(".ad/state/project_skills")
        .join(&state_name);
    let plans = LegacySkillMigrationPlanStore::default();
    let preview = preview_legacy_project_skill_migration(&project, &plans).unwrap();
    let mut changed = std::fs::read(&state_path).unwrap();
    changed.push(b'\n');
    std::fs::write(&state_path, &changed).unwrap();

    assert!(apply_legacy_project_skill_migration(&plans, &migration_claim(&preview)).is_err());
    assert_eq!(std::fs::read(&state_path).unwrap(), changed);
    assert!(
        std::fs::read_dir(home.path().join(".ad/archive/skill-catalog"))
            .unwrap()
            .all(|entry| !entry
                .unwrap()
                .path()
                .to_string_lossy()
                .ends_with(".legacy.json"))
    );
}

#[test]
#[serial(home_env)]
fn migration_requires_a_valid_completed_operation_receipt() {
    let home = tempfile::tempdir().unwrap();
    std::env::set_var("AD_HOME", home.path());
    let (project, _) = ready_legacy_project(home.path(), "missing-receipt");
    let ownership_path = std::fs::read_dir(home.path().join(".ad/state/resource-ownership"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let ownership: serde_json::Value =
        serde_json::from_slice(&std::fs::read(ownership_path).unwrap()).unwrap();
    let receipt_id = ownership["updatedByReceiptId"].as_str().unwrap();
    std::fs::remove_file(
        home.path()
            .join(".ad/history/operations")
            .join(format!("{receipt_id}.json")),
    )
    .unwrap();

    let inventory = inspect_legacy_skill_inventory().unwrap();
    assert_eq!(
        inventory.projects[0].migration_status,
        LegacySkillMigrationStatus::NeedsReconciliation
    );
    assert!(preview_legacy_project_skill_migration(
        &project,
        &LegacySkillMigrationPlanStore::default()
    )
    .is_err());
}

#[test]
#[serial(home_env)]
fn migration_preserves_source_qualified_legacy_intent() {
    let home = tempfile::tempdir().unwrap();
    std::env::set_var("AD_HOME", home.path());
    std::fs::create_dir_all(home.path().join(".claude")).unwrap();
    let project = home.path().join("source-qualified");
    std::fs::create_dir_all(&project).unwrap();
    let project = std::fs::canonicalize(project).unwrap();
    let source_a = home.path().join("source-a");
    let source_b = home.path().join("source-b");
    std::fs::create_dir_all(source_a.join("review")).unwrap();
    std::fs::write(source_a.join("review/SKILL.md"), "# Source A\n").unwrap();
    install_managed_project_skill(home.path(), &project, &source_b);
    write_legacy_source_registry(
        home.path(),
        serde_json::json!([
            {
                "id": "source-a",
                "sourceType": "local",
                "url": source_a,
                "autoUpdate": false,
                "addedAt": "2026-08-01T08:00:00Z"
            },
            {
                "id": "source-b",
                "sourceType": "local",
                "url": source_b,
                "autoUpdate": false,
                "addedAt": "2026-08-01T08:00:00Z"
            }
        ]),
    );
    let state_name = legacy_state_name(&project);
    write_legacy_project_state(
        home.path(),
        &state_name,
        &serde_json::json!({
            "projectPath": project,
            "listedSkills": ["source-a/review"],
            "mode": "allowlist"
        }),
    );

    let inventory = inspect_legacy_skill_inventory().unwrap();
    assert_eq!(
        inventory.projects[0].links[0].source_id.as_deref(),
        Some("source-b")
    );
    assert_eq!(
        inventory.projects[0].migration_status,
        LegacySkillMigrationStatus::NeedsReconciliation
    );
}

#[test]
#[serial(home_env)]
fn migration_requires_exact_allowlist_adoption_without_extra_managed_skills() {
    let home = tempfile::tempdir().unwrap();
    std::env::set_var("AD_HOME", home.path());
    std::fs::create_dir_all(home.path().join(".claude")).unwrap();
    let project = home.path().join("allowlist-extra");
    std::fs::create_dir_all(&project).unwrap();
    let project = std::fs::canonicalize(project).unwrap();
    let source = home.path().join("allowlist-extra-source");
    std::fs::create_dir_all(source.join("extra")).unwrap();
    std::fs::write(source.join("extra/SKILL.md"), "# Extra\n").unwrap();
    install_managed_project_skill(home.path(), &project, &source);
    install_available_project_skill(&project, "extra");
    write_legacy_source_registry(
        home.path(),
        serde_json::json!([{
            "id": "legacy",
            "sourceType": "local",
            "url": source,
            "autoUpdate": false,
            "addedAt": "2026-08-01T08:00:00Z"
        }]),
    );
    let state_name = legacy_state_name(&project);
    write_legacy_project_state(
        home.path(),
        &state_name,
        &serde_json::json!({
            "projectPath": project,
            "listedSkills": ["legacy/review"],
            "mode": "allowlist"
        }),
    );

    let inventory = inspect_legacy_skill_inventory().unwrap();
    assert_eq!(
        inventory.projects[0].migration_status,
        LegacySkillMigrationStatus::NeedsReconciliation
    );
    assert!(preview_legacy_project_skill_migration(
        &project,
        &LegacySkillMigrationPlanStore::default()
    )
    .is_err());
}

#[test]
#[serial(home_env)]
fn migration_rejects_a_symlinked_project_configuration_root() {
    let home = tempfile::tempdir().unwrap();
    std::env::set_var("AD_HOME", home.path());
    let (project, state_name) = ready_legacy_project(home.path(), "symlinked-claude-root");
    let claude_root = project.join(".claude");
    let moved_root = project.join(".claude-real");
    std::fs::rename(&claude_root, &moved_root).unwrap();
    std::os::unix::fs::symlink(&moved_root, &claude_root).unwrap();

    let inventory = inspect_legacy_skill_inventory().unwrap();
    assert_eq!(
        inventory.projects[0].migration_status,
        LegacySkillMigrationStatus::Blocked
    );
    assert!(inventory.projects[0].links.iter().any(|link| {
        link.logical_id == "<skills-root>"
            && link.target_kind == LegacySkillLinkTargetKind::External
    }));
    assert!(preview_legacy_project_skill_migration(
        &project,
        &LegacySkillMigrationPlanStore::default()
    )
    .is_err());
    assert!(home
        .path()
        .join(".ad/state/project_skills")
        .join(state_name)
        .is_file());
}

#[test]
#[serial(home_env)]
fn migration_requires_reconciliation_for_negative_blocklist_intent() {
    let home = tempfile::tempdir().unwrap();
    std::env::set_var("AD_HOME", home.path());
    let (project, state_name) = ready_legacy_project(home.path(), "blocklist-intent");
    write_legacy_project_state(
        home.path(),
        &state_name,
        &serde_json::json!({
            "projectPath": project,
            "listedSkills": ["legacy/review"],
            "mode": "blocklist"
        }),
    );

    let inventory = inspect_legacy_skill_inventory().unwrap();
    assert_eq!(
        inventory.projects[0].migration_status,
        LegacySkillMigrationStatus::NeedsReconciliation
    );
    assert!(preview_legacy_project_skill_migration(
        &project,
        &LegacySkillMigrationPlanStore::default()
    )
    .is_err());
}

#[test]
#[serial(home_env)]
fn restore_rejects_a_marker_that_no_longer_matches_its_receipt() {
    let home = tempfile::tempdir().unwrap();
    std::env::set_var("AD_HOME", home.path());
    let (project, state_name) = ready_legacy_project(home.path(), "marker-binding");
    let state_path = home
        .path()
        .join(".ad/state/project_skills")
        .join(&state_name);
    let plans = LegacySkillMigrationPlanStore::default();
    let preview = preview_legacy_project_skill_migration(&project, &plans).unwrap();
    let report = apply_legacy_project_skill_migration(&plans, &migration_claim(&preview)).unwrap();
    let receipt = report.receipt.unwrap();
    let marker_path = std::fs::read_dir(home.path().join(".ad/archive/skill-catalog"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.to_string_lossy().ends_with(".marker.json"))
        .unwrap();
    let mut marker: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&marker_path).unwrap()).unwrap();
    marker["receiptId"] = serde_json::Value::String("forged-receipt".into());
    std::fs::write(&marker_path, serde_json::to_vec_pretty(&marker).unwrap()).unwrap();

    assert!(rollback_legacy_project_skill_migration(&receipt.id).is_err());
    assert!(!state_path.exists());
    assert!(project.join(".claude/skills/review").is_symlink());
}

#[test]
#[serial(home_env)]
fn migration_rejects_unowned_external_dangling_and_intent_drift_without_cleanup() {
    let home = tempfile::tempdir().unwrap();
    std::env::set_var("AD_HOME", home.path());
    let source = home.path().join("source");
    std::fs::create_dir_all(source.join("review")).unwrap();
    std::fs::write(source.join("review/SKILL.md"), "# Review\n").unwrap();
    write_legacy_source_registry(
        home.path(),
        serde_json::json!([{
            "id": "legacy",
            "sourceType": "local",
            "url": source,
            "autoUpdate": false,
            "addedAt": "2026-08-01T08:00:00Z"
        }]),
    );

    for (name, target_kind) in [
        ("external", LegacySkillLinkTargetKind::External),
        ("dangling", LegacySkillLinkTargetKind::Missing),
        ("directory", LegacySkillLinkTargetKind::External),
    ] {
        let project = home.path().join(name);
        std::fs::create_dir_all(project.join(".claude/skills")).unwrap();
        match name {
            "external" => {
                let external = home.path().join("external-target");
                std::fs::create_dir_all(&external).unwrap();
                std::os::unix::fs::symlink(external, project.join(".claude/skills/review"))
                    .unwrap();
            }
            "dangling" => {
                std::os::unix::fs::symlink(
                    home.path().join("missing-target"),
                    project.join(".claude/skills/review"),
                )
                .unwrap();
            }
            "directory" => {
                std::fs::create_dir_all(project.join(".claude/skills/review")).unwrap();
            }
            _ => unreachable!(),
        }
        let state_name = legacy_state_name(&project);
        write_legacy_project_state(
            home.path(),
            &state_name,
            &serde_json::json!({
                "projectPath": project,
                "listedSkills": if name == "directory" { Vec::<String>::new() } else { vec!["legacy/review".to_owned()] },
                "mode": if name == "directory" { "blocklist" } else { "allowlist" }
            }),
        );
        let inventory = inspect_legacy_skill_inventory().unwrap();
        let view = inventory
            .projects
            .iter()
            .find(|candidate| candidate.project_path == project.to_string_lossy())
            .unwrap();
        assert_ne!(
            view.migration_status,
            LegacySkillMigrationStatus::ReadyToArchive
        );
        assert!(view
            .links
            .iter()
            .any(|link| link.target_kind == target_kind));
        assert!(preview_legacy_project_skill_migration(
            &project,
            &LegacySkillMigrationPlanStore::default()
        )
        .is_err());
        assert!(home
            .path()
            .join(".ad/state/project_skills")
            .join(state_name)
            .exists());
    }
}
