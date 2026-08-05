use std::ffi::OsString;
use std::path::{Path, PathBuf};

use chrono::Utc;
use serial_test::serial;

use super::collection_inventory::{collection_inventory, CollectionObservation};
use super::execution_state::ExecutionState;
use super::*;
use crate::models::{SkillSource, SkillSourceType};

struct EnvironmentGuard {
    ad_home: Option<OsString>,
    codex_home: Option<OsString>,
}

impl EnvironmentGuard {
    fn isolated(home: &Path) -> Self {
        let guard = Self {
            ad_home: std::env::var_os("AD_HOME"),
            codex_home: std::env::var_os("CODEX_HOME"),
        };
        std::env::set_var("AD_HOME", home);
        std::env::remove_var("CODEX_HOME");
        guard
    }
}

impl Drop for EnvironmentGuard {
    fn drop(&mut self) {
        restore_environment("AD_HOME", self.ad_home.take());
        restore_environment("CODEX_HOME", self.codex_home.take());
    }
}

fn restore_environment(name: &str, value: Option<OsString>) {
    match value {
        Some(value) => std::env::set_var(name, value),
        None => std::env::remove_var(name),
    }
}

fn write_skill(source: &Path, body: &str) {
    let skill = source.join("review");
    std::fs::create_dir_all(&skill).unwrap();
    std::fs::write(
        skill.join("SKILL.md"),
        format!("---\nname: review\ndescription: Review code\n---\n{body}\n"),
    )
    .unwrap();
}

fn claude_installation() -> InstallationId {
    builtin_registry()
        .discover()
        .into_iter()
        .find(|installation| installation.agent_id.as_str() == "claude-code")
        .unwrap()
        .id
}

fn add_catalog_source(source: &Path) -> String {
    add_catalog_source_named(source, "Review Skills")
}

fn add_catalog_source_named(source: &Path, display_name: &str) -> String {
    let plans = SkillCatalogPlanStore::default();
    let plan = plans
        .preview_add(SkillSourceRequest {
            display_name: display_name.into(),
            source_type: SkillSourceType::Local,
            location: source.to_string_lossy().into_owned(),
            branch: None,
            subdirectory: None,
            auto_update: false,
        })
        .unwrap();
    let source_id = plan.source_id.clone();
    apply_skill_catalog_plan(
        &plans,
        &SkillCatalogPlanClaim {
            plan_id: plan.id,
            risk_fingerprint: plan.risk_fingerprint,
            confirmed: true,
        },
    )
    .unwrap();
    source_id
}

fn resource_source(resource: &CollectionResourceView) -> &ResourceSourceView {
    resource.provenance.source.as_ref().unwrap()
}

fn update_catalog_source(source_id: &str) -> SkillCatalogOperationReport {
    let plans = SkillCatalogPlanStore::default();
    let plan = plans.preview_update(source_id).unwrap();
    apply_skill_catalog_plan(
        &plans,
        &SkillCatalogPlanClaim {
            plan_id: plan.id,
            risk_fingerprint: plan.risk_fingerprint,
            confirmed: true,
        },
    )
    .unwrap()
}

fn skill<'a>(
    inventory: &'a ProjectWorkspaceInventory,
    logical_id: &str,
) -> &'a CollectionResourceView {
    inventory
        .skills
        .resources
        .iter()
        .find(|resource| resource.logical_id == logical_id)
        .unwrap()
}

fn plugin<'a>(
    inventory: &'a ProjectWorkspaceInventory,
    logical_id: &str,
) -> &'a CollectionResourceView {
    inventory
        .plugins
        .resources
        .iter()
        .find(|resource| resource.logical_id == logical_id)
        .unwrap()
}

fn action_is_available(resource: &CollectionResourceView, action: ResourceAction) -> bool {
    resource.management.actions.iter().any(|candidate| {
        candidate.action == action
            && matches!(
                candidate.availability,
                ResourceActionAvailability::Available
                    | ResourceActionAvailability::ConfirmationRequired
            )
    })
}

fn apply_action(
    installation_id: &InstallationId,
    project: &Path,
    inventory: &ProjectWorkspaceInventory,
    resource: &CollectionResourceView,
    action: ResourceAction,
    plans: &PlanStore,
) -> WorkspaceOperationReport {
    let preview = preview_action(installation_id, project, inventory, resource, action, plans);
    apply_project_collection_action_plan(
        &preview.plan.id,
        &preview.plan.context,
        &preview.plan.risk_fingerprint,
        true,
        plans,
    )
    .unwrap()
}

fn preview_action(
    installation_id: &InstallationId,
    project: &Path,
    inventory: &ProjectWorkspaceInventory,
    resource: &CollectionResourceView,
    action: ResourceAction,
    plans: &PlanStore,
) -> ProjectCollectionActionPreview {
    preview_project_collection_action(
        installation_id,
        project,
        ProjectCollectionActionRequest {
            workspace_key: inventory.workspace.key.clone(),
            inventory_revision: inventory.revision.clone(),
            resource_key: resource.key.clone(),
            action,
        },
        plans,
    )
    .unwrap()
}

fn setup() -> (
    tempfile::TempDir,
    EnvironmentGuard,
    PathBuf,
    PathBuf,
    PathBuf,
) {
    let home = tempfile::tempdir().unwrap();
    let guard = EnvironmentGuard::isolated(home.path());
    let source = home.path().join("source");
    let project_a = home.path().join("project-a");
    let project_b = home.path().join("project-b");
    std::fs::create_dir_all(home.path().join(".claude")).unwrap();
    std::fs::create_dir_all(&project_a).unwrap();
    std::fs::create_dir_all(&project_b).unwrap();
    write_skill(&source, "first revision");
    (home, guard, source, project_a, project_b)
}

#[test]
#[serial(home_env)]
fn catalog_local_skill_actions_share_the_original_live_source() {
    let (_home, _guard, source, project_a, project_b) = setup();
    let source_id = add_catalog_source(&source);
    let installation_id = claude_installation();
    let plans = PlanStore::default();

    let inventory_a = inspect_project_workspace_inventory(&installation_id, &project_a).unwrap();
    let inventory_b = inspect_project_workspace_inventory(&installation_id, &project_b).unwrap();
    assert_eq!(
        skill(&inventory_a, "review").effective_state,
        EffectiveResourceState::Unconfigured
    );
    assert_eq!(
        skill(&inventory_b, "review").effective_state,
        EffectiveResourceState::Unconfigured
    );
    assert!(action_is_available(
        skill(&inventory_a, "review"),
        ResourceAction::Install
    ));
    assert_eq!(
        resource_source(skill(&inventory_a, "review")),
        &ResourceSourceView {
            kind: ResourceSourceKind::CatalogLocal,
            display_name: "Review Skills".into(),
            location: source.to_string_lossy().into_owned(),
            branch: None,
            subdirectory: None,
        }
    );

    apply_action(
        &installation_id,
        &project_a,
        &inventory_a,
        skill(&inventory_a, "review"),
        ResourceAction::Install,
        &plans,
    );
    apply_action(
        &installation_id,
        &project_b,
        &inventory_b,
        skill(&inventory_b, "review"),
        ResourceAction::Install,
        &plans,
    );
    let link_a = project_a.join(".claude/skills/review");
    let link_b = project_b.join(".claude/skills/review");
    let first_a = std::fs::read_link(&link_a).unwrap();
    let first_b = std::fs::read_link(&link_b).unwrap();
    assert_eq!(first_a, first_b);
    assert_eq!(
        first_a,
        std::fs::canonicalize(source.join("review")).unwrap()
    );

    write_skill(&source, "second revision");
    assert_eq!(std::fs::read_link(&link_a).unwrap(), first_a);
    assert_eq!(std::fs::read_link(&link_b).unwrap(), first_b);
    assert!(std::fs::read_to_string(link_a.join("SKILL.md"))
        .unwrap()
        .contains("second revision"));
    assert!(std::fs::read_to_string(link_b.join("SKILL.md"))
        .unwrap()
        .contains("second revision"));

    let source_update = update_catalog_source(&source_id);
    let source_receipt = source_update.receipt.unwrap();
    assert_eq!(source_receipt.affected_resources.len(), 2);
    assert_eq!(source_receipt.affected_workspaces.len(), 2);
    let inventory_a = inspect_project_workspace_inventory(&installation_id, &project_a).unwrap();
    let inventory_b = inspect_project_workspace_inventory(&installation_id, &project_b).unwrap();
    assert_eq!(
        resource_source(skill(&inventory_a, "review")).kind,
        ResourceSourceKind::CatalogLocal
    );
    assert_eq!(
        resource_source(skill(&inventory_a, "review")).location,
        source.to_string_lossy()
    );
    assert!(!action_is_available(
        skill(&inventory_a, "review"),
        ResourceAction::Update
    ));
    assert!(!action_is_available(
        skill(&inventory_b, "review"),
        ResourceAction::Update
    ));

    let inventory_a = inspect_project_workspace_inventory(&installation_id, &project_a).unwrap();
    apply_action(
        &installation_id,
        &project_a,
        &inventory_a,
        skill(&inventory_a, "review"),
        ResourceAction::Remove,
        &plans,
    );
    assert!(std::fs::symlink_metadata(&link_a).is_err());
    assert_eq!(std::fs::read_link(&link_b).unwrap(), first_b);
    let inventory_a = inspect_project_workspace_inventory(&installation_id, &project_a).unwrap();
    let inventory_b = inspect_project_workspace_inventory(&installation_id, &project_b).unwrap();
    assert_eq!(
        skill(&inventory_a, "review").effective_state,
        EffectiveResourceState::Unconfigured
    );
    assert_eq!(std::fs::read_link(&link_b).unwrap(), first_b);
    assert_eq!(
        resource_source(skill(&inventory_b, "review")).kind,
        ResourceSourceKind::CatalogLocal
    );

    let source_plans = SkillCatalogPlanStore::default();
    let blocked_remove = source_plans.preview_remove(&source_id).unwrap();
    assert_eq!(
        blocked_remove.applicability,
        SkillCatalogPlanApplicability::Blocked
    );
    assert_eq!(blocked_remove.affected_workspaces.len(), 1);
    assert!(apply_skill_catalog_plan(
        &source_plans,
        &SkillCatalogPlanClaim {
            plan_id: blocked_remove.id,
            risk_fingerprint: blocked_remove.risk_fingerprint,
            confirmed: true,
        },
    )
    .is_err());

    std::fs::remove_file(&link_b).unwrap();
    std::os::unix::fs::symlink(source.join("review/scripts"), &link_b).unwrap();
    let tampered = inspect_project_workspace_inventory(&installation_id, &project_b).unwrap();
    assert!(!action_is_available(
        skill(&tampered, "review"),
        ResourceAction::Remove
    ));
    std::fs::remove_file(&link_b).unwrap();
    std::os::unix::fs::symlink(&first_b, &link_b).unwrap();

    let inventory_b = inspect_project_workspace_inventory(&installation_id, &project_b).unwrap();
    apply_action(
        &installation_id,
        &project_b,
        &inventory_b,
        skill(&inventory_b, "review"),
        ResourceAction::Remove,
        &plans,
    );
    let removable = source_plans.preview_remove(&source_id).unwrap();
    assert_eq!(
        removable.applicability,
        SkillCatalogPlanApplicability::Applicable
    );
    apply_skill_catalog_plan(
        &source_plans,
        &SkillCatalogPlanClaim {
            plan_id: removable.id,
            risk_fingerprint: removable.risk_fingerprint,
            confirmed: true,
        },
    )
    .unwrap();
}

#[test]
#[serial(home_env)]
fn legacy_artifact_link_relinks_explicitly_and_rolls_back_exactly() {
    let (_home, _guard, source, project_a, _project_b) = setup();
    let source_id = format!("skill-source:{}", uuid::Uuid::new_v4());
    let acquisition = SkillSource {
        id: source_id.clone(),
        source_type: SkillSourceType::Local,
        url: source.to_string_lossy().into_owned(),
        branch: None,
        subdirectory: None,
        auto_update: false,
        added_at: Utc::now(),
    };
    let artifact =
        publish_staged_skill_artifact(stage_skill_source(&acquisition).unwrap()).unwrap();
    let artifact_skill = verify_skill_artifact(&artifact).unwrap().join("review");
    let catalog = serde_json::to_vec_pretty(&serde_json::json!({
        "schemaVersion": 1,
        "entries": [{
            "sourceId": source_id,
            "displayName": "Review Skills",
            "sourceType": "local",
            "location": source.to_string_lossy(),
            "autoUpdate": false,
            "currentArtifact": artifact,
            "addedAt": Utc::now(),
            "updatedAt": Utc::now(),
        }],
    }))
    .unwrap();
    let state = ExecutionState::open().unwrap();
    state
        .state()
        .write_atomic("skill_catalog.json", &catalog)
        .unwrap();
    let installation_id = claude_installation();
    let plans = PlanStore::default();
    let inventory = inspect_project_workspace_inventory(&installation_id, &project_a).unwrap();
    apply_action(
        &installation_id,
        &project_a,
        &inventory,
        skill(&inventory, "review"),
        ResourceAction::Install,
        &plans,
    );
    let installed = project_a.join(".claude/skills/review");
    assert_eq!(std::fs::read_link(&installed).unwrap(), artifact_skill);

    let source_update = update_catalog_source(&acquisition.id);
    assert_eq!(source_update.receipt.unwrap().affected_workspaces.len(), 1);
    let inventory = inspect_project_workspace_inventory(&installation_id, &project_a).unwrap();
    let legacy = skill(&inventory, "review");
    let update = legacy
        .management
        .actions
        .iter()
        .find(|action| action.action == ResourceAction::Update)
        .unwrap();
    assert_eq!(update.intent, ResourceActionIntent::Relink);
    let relink = apply_action(
        &installation_id,
        &project_a,
        &inventory,
        legacy,
        ResourceAction::Update,
        &plans,
    );
    let relink_receipt = relink.receipt.unwrap();
    assert_eq!(
        std::fs::read_link(&installed).unwrap(),
        std::fs::canonicalize(source.join("review")).unwrap()
    );

    let rollback_plans = PlanStore::default();
    let rollback = ExecutionEngine
        .preview_rollback_bound(
            &relink_receipt.id,
            &relink_receipt.context.clone().unwrap(),
            &rollback_plans,
        )
        .unwrap();
    ExecutionEngine
        .apply_acknowledged_bound(
            &rollback.id,
            &rollback.context,
            &rollback.risk_fingerprint,
            &rollback_plans,
            &[PlanAcknowledgement {
                code: PlanAcknowledgementCode::RollbackApply,
                accepted: true,
            }],
        )
        .unwrap();

    assert_eq!(std::fs::read_link(&installed).unwrap(), artifact_skill);
    let restored = inspect_project_workspace_inventory(&installation_id, &project_a).unwrap();
    assert_eq!(
        skill(&restored, "review")
            .management
            .actions
            .iter()
            .find(|action| action.action == ResourceAction::Update)
            .unwrap()
            .intent,
        ResourceActionIntent::Relink
    );
}

#[test]
#[serial(home_env)]
fn source_update_rejects_ownership_changes_after_preview() {
    let (_home, _guard, source, project_a, project_b) = setup();
    let source_id = add_catalog_source(&source);
    let installation_id = claude_installation();
    let action_plans = PlanStore::default();
    let inventory_a = inspect_project_workspace_inventory(&installation_id, &project_a).unwrap();
    apply_action(
        &installation_id,
        &project_a,
        &inventory_a,
        skill(&inventory_a, "review"),
        ResourceAction::Install,
        &action_plans,
    );
    let source_plans = SkillCatalogPlanStore::default();
    let source_update = source_plans.preview_update(&source_id).unwrap();
    assert_eq!(source_update.affected_workspaces.len(), 1);

    let inventory_b = inspect_project_workspace_inventory(&installation_id, &project_b).unwrap();
    apply_action(
        &installation_id,
        &project_b,
        &inventory_b,
        skill(&inventory_b, "review"),
        ResourceAction::Install,
        &action_plans,
    );
    let error = apply_skill_catalog_plan(
        &source_plans,
        &SkillCatalogPlanClaim {
            plan_id: source_update.id,
            risk_fingerprint: source_update.risk_fingerprint,
            confirmed: true,
        },
    )
    .unwrap_err();

    assert!(matches!(
        error,
        SkillCatalogExecutionError::Plan(SkillCatalogPlanError::RiskChanged)
    ));
}

#[test]
#[serial(home_env)]
fn stale_inventory_and_external_skill_never_become_mutation_authority() {
    let (_home, _guard, source, project_a, _project_b) = setup();
    add_catalog_source(&source);
    let installation_id = claude_installation();
    let plans = PlanStore::default();
    let stale = inspect_project_workspace_inventory(&installation_id, &project_a).unwrap();
    let preview = preview_action(
        &installation_id,
        &project_a,
        &stale,
        skill(&stale, "review"),
        ResourceAction::Install,
        &plans,
    );
    assert_eq!(
        preview.plan.required_acknowledgements,
        vec![AcknowledgementRequirement {
            code: PlanAcknowledgementCode::ProjectCollectionApply,
            risk: PlanRiskLevel::Confirmation,
        }]
    );
    let error = apply_project_collection_action_plan(
        &preview.plan.id,
        &preview.plan.context,
        &preview.plan.risk_fingerprint,
        false,
        &plans,
    )
    .unwrap_err();
    assert_eq!(error.code, AgentErrorCode::ConfirmationRequired);
    apply_project_collection_action_plan(
        &preview.plan.id,
        &preview.plan.context,
        &preview.plan.risk_fingerprint,
        true,
        &plans,
    )
    .unwrap();

    let error = preview_project_collection_action(
        &installation_id,
        &project_a,
        ProjectCollectionActionRequest {
            workspace_key: stale.workspace.key.clone(),
            inventory_revision: stale.revision.clone(),
            resource_key: skill(&stale, "review").key.clone(),
            action: ResourceAction::Install,
        },
        &plans,
    )
    .unwrap_err();
    assert_eq!(error.code, AgentErrorCode::ResourceChanged);

    let external_path = project_a.join(".claude/skills/external");
    std::fs::create_dir_all(&external_path).unwrap();
    std::fs::write(external_path.join("SKILL.md"), "# External\n").unwrap();
    let canonical_external_path = std::fs::canonicalize(&external_path).unwrap();
    let inventory = inspect_project_workspace_inventory(&installation_id, &project_a).unwrap();
    let external = skill(&inventory, "external");
    assert_eq!(external.ownership.kind, ResourceOwnershipKind::External);
    assert!(!action_is_available(external, ResourceAction::Remove));
    assert_eq!(
        resource_source(external),
        &ResourceSourceView {
            kind: ResourceSourceKind::InstalledPath,
            display_name: "external".into(),
            location: canonical_external_path.to_string_lossy().into_owned(),
            branch: None,
            subdirectory: None,
        }
    );
}

#[test]
#[serial(home_env)]
fn same_named_catalog_skills_are_distinct_conflict_sources() {
    let (home, _guard, source, project_a, _project_b) = setup();
    let peer_source = home.path().join("peer-source");
    write_skill(&peer_source, "peer revision");
    add_catalog_source_named(&source, "Primary Skills");
    add_catalog_source_named(&peer_source, "Peer Skills");
    let installation_id = claude_installation();

    let inventory = inspect_project_workspace_inventory(&installation_id, &project_a).unwrap();
    let resources = inventory
        .skills
        .resources
        .iter()
        .filter(|resource| resource.logical_id == "review")
        .collect::<Vec<_>>();

    assert_eq!(resources.len(), 2);
    assert!(resources
        .iter()
        .all(|resource| resource.effective_state == EffectiveResourceState::Conflict));
    assert_eq!(
        resources
            .iter()
            .map(|resource| resource_source(resource).location.clone())
            .collect::<std::collections::BTreeSet<_>>(),
        std::collections::BTreeSet::from([
            source.to_string_lossy().into_owned(),
            peer_source.to_string_lossy().into_owned(),
        ])
    );
}

#[test]
#[serial(home_env)]
fn git_catalog_source_is_redacted_at_the_inventory_boundary() {
    let (home, _guard, source, project_a, _project_b) = setup();
    let source_id = add_catalog_source(&source);
    let catalog_path = home.path().join(".ad/state/skill_catalog.json");
    let mut catalog: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&catalog_path).unwrap()).unwrap();
    let entry = catalog["entries"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|entry| entry["sourceId"] == source_id)
        .unwrap();
    entry["sourceType"] = serde_json::json!("git");
    entry["location"] =
        serde_json::json!("https://user:password@example.com/team/skills.git?token=hidden#review");
    entry["branch"] = serde_json::json!("stable");
    entry["subdirectory"] = serde_json::json!("skills/review");
    std::fs::write(&catalog_path, serde_json::to_vec_pretty(&catalog).unwrap()).unwrap();

    let inventory =
        inspect_project_workspace_inventory(&claude_installation(), &project_a).unwrap();
    let source = resource_source(skill(&inventory, "review"));

    assert_eq!(source.kind, ResourceSourceKind::CatalogGit);
    assert_eq!(source.location, "https://example.com/team/skills.git");
    assert_eq!(source.branch.as_deref(), Some("stable"));
    assert_eq!(source.subdirectory.as_deref(), Some("skills/review"));
}

#[test]
#[serial(home_env)]
fn contradictory_source_metadata_fails_closed_without_hiding_resource() {
    let (_home, _guard, _source, project_a, _project_b) = setup();
    let installation_id = claude_installation();
    let workspace = resolve_project_agent_workspace(&installation_id, &project_a).unwrap();
    let version_diagnostic = ItemDiagnostic {
        code: "agent_version_unverified".into(),
        message_key: "agents.inventory.agentVersionUnverified".into(),
        retryable: false,
        resource_key: None,
    };
    let resource = ResourceRef {
        installation_id: workspace.effective_installation_id.clone(),
        project_path: Some(workspace.canonical_project_path.clone()),
        kind: ResourceKind::Skills,
        scope: ResourceScope::Project,
        logical_id: "review".into(),
    };
    let observation = |kind: ResourceSourceKind, location: &str| CollectionObservation {
        resource: resource.clone(),
        layer: ResourceLayer::Project,
        source_id: "skill-source:shared".into(),
        target_id: PhysicalTargetId::for_resource(&resource),
        logical_id: "review".into(),
        display_name: "review".into(),
        description: None,
        enabled: false,
        ownership: ResourceOwnershipKind::AdManaged,
        ownership_record: None,
        health: ResourceHealthView {
            status: ResourceHealthStatus::Healthy,
            diagnostic: None,
        },
        configured: false,
        artifact_id: Some("artifact".into()),
        resettable: false,
        source: Some(ResourceSourceView {
            kind,
            display_name: "Shared Skills".into(),
            location: location.into(),
            branch: None,
            subdirectory: None,
        }),
    };

    let inventory = collection_inventory(
        &workspace,
        ResourceKind::Skills,
        vec![
            observation(ResourceSourceKind::CatalogLocal, "/catalog/a"),
            observation(ResourceSourceKind::CatalogLocal, "/catalog/b"),
        ],
        &version_diagnostic,
        Vec::new(),
    );

    assert_eq!(inventory.resources.len(), 1);
    assert!(inventory.resources[0].provenance.source.is_none());
    assert!(inventory
        .coverage
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "resource_source_conflict"
            && diagnostic.resource_key.as_ref() == Some(&inventory.resources[0].key)));

    let inventory = collection_inventory(
        &workspace,
        ResourceKind::Skills,
        vec![
            observation(ResourceSourceKind::InstalledPath, "/installed/a"),
            observation(ResourceSourceKind::InstalledPath, "/installed/b"),
        ],
        &version_diagnostic,
        Vec::new(),
    );

    assert_eq!(inventory.resources.len(), 1);
    assert!(inventory.resources[0].provenance.source.is_none());
    assert!(inventory
        .coverage
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "resource_source_conflict"
            && diagnostic.resource_key.as_ref() == Some(&inventory.resources[0].key)));
}

#[test]
#[serial(home_env)]
fn collection_action_receipt_supports_guarded_rollback() {
    let (_home, _guard, source, project_a, _project_b) = setup();
    add_catalog_source(&source);
    let installation_id = claude_installation();
    let inventory = inspect_project_workspace_inventory(&installation_id, &project_a).unwrap();
    let plans = PlanStore::default();
    let report = apply_action(
        &installation_id,
        &project_a,
        &inventory,
        skill(&inventory, "review"),
        ResourceAction::Install,
        &plans,
    );
    let receipt = report.receipt.unwrap();
    assert!(receipt
        .action_id
        .as_deref()
        .is_some_and(|action| action.starts_with("install:")));
    let rollback_plans = PlanStore::default();
    let rollback = ExecutionEngine
        .preview_rollback_bound(
            &receipt.id,
            &receipt.context.clone().unwrap(),
            &rollback_plans,
        )
        .unwrap();
    ExecutionEngine
        .apply_acknowledged_bound(
            &rollback.id,
            &rollback.context,
            &rollback.risk_fingerprint,
            &rollback_plans,
            &[PlanAcknowledgement {
                code: PlanAcknowledgementCode::RollbackApply,
                accepted: true,
            }],
        )
        .unwrap();
    assert!(std::fs::symlink_metadata(project_a.join(".claude/skills/review")).is_err());
}

#[test]
#[serial(home_env)]
fn plugin_override_actions_never_mutate_user_or_peer_project_state() {
    let (home, _guard, _source, project_a, project_b) = setup();
    std::fs::write(
        home.path().join(".claude/settings.json"),
        br#"{"enabledPlugins":{"demo":true}}"#,
    )
    .unwrap();
    std::fs::create_dir_all(project_a.join(".claude")).unwrap();
    std::fs::create_dir_all(project_b.join(".claude")).unwrap();
    std::fs::write(
        project_a.join(".claude/settings.local.json"),
        br#"{"enabledPlugins":{"demo":false,"keep":true},"unknown":1}"#,
    )
    .unwrap();
    std::fs::write(
        project_b.join(".claude/settings.json"),
        br#"{"enabledPlugins":{"demo":false}}"#,
    )
    .unwrap();
    let installation_id = claude_installation();
    let plans = PlanStore::default();
    let inventory_a = inspect_project_workspace_inventory(&installation_id, &project_a).unwrap();
    let inventory_b = inspect_project_workspace_inventory(&installation_id, &project_b).unwrap();
    assert_eq!(
        plugin(&inventory_a, "demo").effective_state,
        EffectiveResourceState::Disabled
    );
    assert!(action_is_available(
        plugin(&inventory_a, "demo"),
        ResourceAction::Enable
    ));
    assert!(action_is_available(
        plugin(&inventory_a, "demo"),
        ResourceAction::Remove
    ));
    assert!(!action_is_available(
        plugin(&inventory_b, "demo"),
        ResourceAction::Remove
    ));
    assert!(plugin(&inventory_a, "demo").provenance.source.is_none());

    apply_action(
        &installation_id,
        &project_a,
        &inventory_a,
        plugin(&inventory_a, "demo"),
        ResourceAction::Enable,
        &plans,
    );
    let local_path = project_a.join(".claude/settings.local.json");
    let local: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&local_path).unwrap()).unwrap();
    assert_eq!(local["enabledPlugins"]["demo"], true);
    assert_eq!(local["unknown"], 1);
    assert!(!project_b.join(".claude/settings.local.json").exists());
    let user_before = std::fs::read(home.path().join(".claude/settings.json")).unwrap();

    let inventory_a = inspect_project_workspace_inventory(&installation_id, &project_a).unwrap();
    apply_action(
        &installation_id,
        &project_a,
        &inventory_a,
        plugin(&inventory_a, "demo"),
        ResourceAction::Remove,
        &plans,
    );
    let local: serde_json::Value =
        serde_json::from_slice(&std::fs::read(local_path).unwrap()).unwrap();
    assert!(local["enabledPlugins"].get("demo").is_none());
    assert_eq!(local["enabledPlugins"]["keep"], true);
    assert_eq!(local["unknown"], 1);
    assert_eq!(
        std::fs::read(home.path().join(".claude/settings.json")).unwrap(),
        user_before
    );
    assert!(!project_b.join(".claude/settings.local.json").exists());
}
