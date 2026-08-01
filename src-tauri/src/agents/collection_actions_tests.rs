use std::ffi::OsString;
use std::path::{Path, PathBuf};

use serial_test::serial;

use super::*;
use crate::models::SkillSourceType;

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
    let plans = SkillCatalogPlanStore::default();
    let plan = plans
        .preview_add(SkillSourceRequest {
            display_name: "Review Skills".into(),
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

fn update_catalog_source(source_id: &str) {
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
    .unwrap();
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
fn catalog_skill_actions_keep_project_revisions_isolated() {
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

    write_skill(&source, "second revision");
    update_catalog_source(&source_id);
    let inventory_a = inspect_project_workspace_inventory(&installation_id, &project_a).unwrap();
    let inventory_b = inspect_project_workspace_inventory(&installation_id, &project_b).unwrap();
    assert!(action_is_available(
        skill(&inventory_a, "review"),
        ResourceAction::Update
    ));
    assert!(action_is_available(
        skill(&inventory_b, "review"),
        ResourceAction::Update
    ));

    apply_action(
        &installation_id,
        &project_a,
        &inventory_a,
        skill(&inventory_a, "review"),
        ResourceAction::Update,
        &plans,
    );
    let second_a = std::fs::read_link(&link_a).unwrap();
    assert_ne!(second_a, first_a);
    assert_eq!(std::fs::read_link(&link_b).unwrap(), first_b);
    assert!(second_a.join("SKILL.md").is_file());
    assert!(first_b.join("SKILL.md").is_file());

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
    assert!(action_is_available(
        skill(&inventory_b, "review"),
        ResourceAction::Update
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

    let external = project_a.join(".claude/skills/external");
    std::fs::create_dir_all(&external).unwrap();
    std::fs::write(external.join("SKILL.md"), "# External\n").unwrap();
    let inventory = inspect_project_workspace_inventory(&installation_id, &project_a).unwrap();
    let external = skill(&inventory, "external");
    assert_eq!(external.ownership.kind, ResourceOwnershipKind::External);
    assert!(!action_is_available(external, ResourceAction::Remove));
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
