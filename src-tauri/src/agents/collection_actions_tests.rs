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
    write_skill_named(source, "review", "Review code", body);
}

fn write_skill_named(source: &Path, name: &str, description: &str, body: &str) {
    let skill = source.join(name);
    std::fs::create_dir_all(&skill).unwrap();
    std::fs::write(
        skill.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: {description}\n---\n{body}\n"),
    )
    .unwrap();
}

fn write_plugin(source: &Path) {
    write_plugin_named(source, "native-plugin");
}

fn write_plugin_named(source: &Path, name: &str) {
    let plugin = source.join(name);
    std::fs::create_dir_all(plugin.join(".claude-plugin")).unwrap();
    std::fs::create_dir_all(plugin.join("commands")).unwrap();
    std::fs::write(
        plugin.join(".claude-plugin/plugin.json"),
        format!(r#"{{"name":"{name}","description":"Native Plugin"}}"#),
    )
    .unwrap();
    std::fs::write(plugin.join("commands/demo.md"), "# Demo\n").unwrap();
}

fn write_codex_plugin_named(source: &Path, name: &str, marketplace: &str, version: &str) {
    let plugin = source.join(name);
    std::fs::create_dir_all(plugin.join(".codex-plugin")).unwrap();
    std::fs::create_dir_all(plugin.join(".agents/plugins")).unwrap();
    std::fs::create_dir_all(plugin.join("skills/demo")).unwrap();
    std::fs::write(
        plugin.join(".codex-plugin/plugin.json"),
        format!(r#"{{"name":"{name}","version":"{version}"}}"#),
    )
    .unwrap();
    std::fs::write(
        plugin.join(".agents/plugins/marketplace.json"),
        format!(
            r#"{{"name":"{marketplace}","plugins":[{{"name":"{name}","source":{{"source":"local","path":"./"}}}}]}}"#
        ),
    )
    .unwrap();
    std::fs::write(
        plugin.join("skills/demo/SKILL.md"),
        "---\nname: demo\ndescription: Demo\n---\n",
    )
    .unwrap();
}

fn add_root_git_plugin_source() -> String {
    let source = SkillSource {
        id: format!("skill-source:{}", uuid::Uuid::new_v4()),
        source_type: SkillSourceType::Git,
        url: "https://example.com/team/root-plugin.git".into(),
        branch: None,
        subdirectory: None,
        auto_update: false,
        added_at: Utc::now(),
    };
    let operation = crate::fs::paths::skill_acquisition_staging_dir()
        .unwrap()
        .join(uuid::Uuid::new_v4().to_string());
    std::fs::create_dir_all(operation.join("source/.claude-plugin")).unwrap();
    std::fs::write(
        operation.join("source/.claude-plugin/plugin.json"),
        r#"{"name":"root-plugin","description":"Root Plugin"}"#,
    )
    .unwrap();
    let staged = super::skill_source_bindings::stage_existing_git_checkout_for_test(
        &source,
        operation,
        &"a".repeat(40),
    )
    .unwrap();
    let (binding, publication) = publish_staged_git_skill_source_binding(staged, None).unwrap();
    publication.commit();
    let request = SkillSourceRequest {
        display_name: "Root Plugin".into(),
        source_type: SkillSourceType::Git,
        location: source.url.clone(),
        branch: None,
        subdirectory: None,
        auto_update: false,
    };
    let mut document = super::skill_catalog::SkillCatalogDocument::empty();
    document
        .add_binding(source.id.clone(), &request, binding, Utc::now())
        .unwrap();
    let state = ExecutionState::open().unwrap();
    state
        .state()
        .write_atomic("skill_catalog.json", &document.render().unwrap())
        .unwrap();
    super::resource_catalog::persist_resource_catalog_projection(
        state.state(),
        &super::skill_catalog::load_skill_catalog_state_from(state.state())
            .unwrap()
            .snapshot(),
    )
    .unwrap();
    source.id
}

fn claude_installation() -> InstallationId {
    builtin_registry()
        .discover()
        .into_iter()
        .find(|installation| installation.agent_id.as_str() == "claude-code")
        .unwrap()
        .id
}

fn codex_installation() -> InstallationId {
    builtin_registry()
        .discover()
        .into_iter()
        .find(|installation| installation.agent_id.as_str() == "codex")
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
    let installations = list_resource_installations().unwrap();
    assert_eq!(installations.len(), 2);
    assert!(installations.iter().all(|installation| {
        installation.resource_kind == ResourceKind::Skills
            && installation.install_id == "review"
            && installation.source_id == source_id
    }));
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
    assert_eq!(list_resource_installations().unwrap().len(), 1);
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
    assert!(list_resource_installations().unwrap().is_empty());
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
fn catalog_source_batch_install_previews_and_applies_one_combined_plan() {
    let (_home, _guard, source, project_a, _project_b) = setup();
    write_skill_named(&source, "format", "Format code", "format instructions");
    add_catalog_source(&source);
    let installation_id = claude_installation();
    let plans = PlanStore::default();
    let inventory = inspect_project_workspace_inventory(&installation_id, &project_a).unwrap();
    let anchor = skill(&inventory, "review");

    let preview = preview_project_collection_source_install(
        &installation_id,
        &project_a,
        ProjectCollectionSourceInstallRequest {
            workspace_key: inventory.workspace.key.clone(),
            inventory_revision: inventory.revision.clone(),
            source_resource_key: anchor.key.clone(),
        },
        &plans,
    )
    .unwrap();

    assert_eq!(preview.source, resource_source(anchor).clone());
    assert_eq!(preview.resource_keys.len(), 2);
    assert_eq!(preview.plan.changes.len(), 2);
    let report = apply_project_collection_action_plan(
        &preview.plan.id,
        &preview.plan.context,
        &preview.plan.risk_fingerprint,
        true,
        &plans,
    )
    .unwrap();
    assert_eq!(report.outcome, WorkspaceOperationOutcome::Changed);
    assert_eq!(report.receipt.as_ref().unwrap().applied_resources.len(), 2);
    assert!(project_a.join(".claude/skills/review").is_symlink());
    assert!(project_a.join(".claude/skills/format").is_symlink());

    let installed = inspect_project_workspace_inventory(&installation_id, &project_a).unwrap();
    assert!(!action_is_available(
        skill(&installed, "review"),
        ResourceAction::Install
    ));
    assert!(!action_is_available(
        skill(&installed, "format"),
        ResourceAction::Install
    ));
}

#[test]
#[serial(home_env)]
fn catalog_claude_plugin_installs_as_one_link_and_decorates_only_its_project_launch() {
    let (_home, _guard, source, project_a, project_b) = setup();
    write_plugin(&source);
    add_catalog_source(&source);
    let installation_id = claude_installation();
    let plans = PlanStore::default();
    let inventory_a = inspect_project_workspace_inventory(&installation_id, &project_a).unwrap();
    let candidate = plugin(&inventory_a, "native-plugin");
    assert!(action_is_available(candidate, ResourceAction::Install));

    apply_action(
        &installation_id,
        &project_a,
        &inventory_a,
        candidate,
        ResourceAction::Install,
        &plans,
    );

    let context_a = AgentContext {
        installation_id: installation_id.clone(),
        project_path: Some(
            std::fs::canonicalize(&project_a)
                .unwrap()
                .to_string_lossy()
                .into(),
        ),
    };
    let context_b = AgentContext {
        installation_id,
        project_path: Some(
            std::fs::canonicalize(&project_b)
                .unwrap()
                .to_string_lossy()
                .into(),
        ),
    };
    let recipe_a = builtin_registry()
        .adapter("claude-code")
        .unwrap()
        .launcher()
        .unwrap()
        .recipe(&context_a)
        .unwrap();
    let recipe_b = builtin_registry()
        .adapter("claude-code")
        .unwrap()
        .launcher()
        .unwrap()
        .recipe(&context_b)
        .unwrap();
    assert_eq!(recipe_a.args.len(), 2);
    assert_eq!(recipe_a.args[0], "--plugin-dir");
    let link = PathBuf::from(&recipe_a.args[1]);
    assert!(std::fs::symlink_metadata(&link)
        .unwrap()
        .file_type()
        .is_symlink());
    assert_eq!(
        std::fs::read_link(&link).unwrap(),
        std::fs::canonicalize(source.join("native-plugin")).unwrap()
    );
    assert!(recipe_b.args.is_empty());

    let installed = inspect_project_workspace_inventory(
        &context_a.installation_id,
        Path::new(context_a.project_path.as_deref().unwrap()),
    )
    .unwrap();
    assert!(action_is_available(
        plugin(&installed, "native-plugin"),
        ResourceAction::Disable
    ));
    apply_action(
        &context_a.installation_id,
        Path::new(context_a.project_path.as_deref().unwrap()),
        &installed,
        plugin(&installed, "native-plugin"),
        ResourceAction::Disable,
        &plans,
    );
    let disabled = inspect_project_workspace_inventory(
        &context_a.installation_id,
        Path::new(context_a.project_path.as_deref().unwrap()),
    )
    .unwrap();
    assert_eq!(
        plugin(&disabled, "native-plugin").effective_state,
        EffectiveResourceState::Disabled
    );
    assert!(builtin_registry()
        .adapter("claude-code")
        .unwrap()
        .launcher()
        .unwrap()
        .recipe(&context_a)
        .unwrap()
        .args
        .is_empty());
    apply_action(
        &context_a.installation_id,
        Path::new(context_a.project_path.as_deref().unwrap()),
        &disabled,
        plugin(&disabled, "native-plugin"),
        ResourceAction::Enable,
        &plans,
    );
    assert_eq!(
        builtin_registry()
            .adapter("claude-code")
            .unwrap()
            .launcher()
            .unwrap()
            .recipe(&context_a)
            .unwrap()
            .args,
        recipe_a.args
    );

    let installed = inspect_project_workspace_inventory(
        &context_a.installation_id,
        Path::new(context_a.project_path.as_deref().unwrap()),
    )
    .unwrap();
    apply_action(
        &context_a.installation_id,
        Path::new(context_a.project_path.as_deref().unwrap()),
        &installed,
        plugin(&installed, "native-plugin"),
        ResourceAction::Remove,
        &plans,
    );
    assert!(std::fs::symlink_metadata(link).is_err());
    assert!(builtin_registry()
        .adapter("claude-code")
        .unwrap()
        .launcher()
        .unwrap()
        .recipe(&context_a)
        .unwrap()
        .args
        .is_empty());
}

#[test]
#[serial(home_env)]
fn root_level_git_plugin_installs_from_the_stable_checkout_link() {
    let (_home, _guard, _source, project_a, _project_b) = setup();
    let source_id = add_root_git_plugin_source();
    let installation_id = claude_installation();
    let inventory = inspect_project_workspace_inventory(&installation_id, &project_a).unwrap();
    let candidate = plugin(&inventory, "root-plugin");

    apply_action(
        &installation_id,
        &project_a,
        &inventory,
        candidate,
        ResourceAction::Install,
        &PlanStore::default(),
    );

    let record = list_resource_installations().unwrap().pop().unwrap();
    assert_eq!(record.source_id, source_id);
    let target = PathBuf::from(
        load_ownership_record_by_id(
            &ExecutionState::open().unwrap(),
            &record.ownership_record_ids[0],
        )
        .unwrap()
        .unwrap()
        .target_path,
    );
    let catalog = load_resource_catalog_snapshot().unwrap();
    let stable_root = PathBuf::from(
        &catalog.sources[&record.source_id]
            .binding
            .as_ref()
            .unwrap()
            .stable_root,
    );
    assert_eq!(std::fs::read_link(target).unwrap(), stable_root);
}

#[test]
#[serial(home_env)]
fn unavailable_catalog_plugin_does_not_hide_healthy_plugins() {
    let (_home, _guard, source, project_a, _project_b) = setup();
    write_plugin_named(&source, "broken-plugin");
    write_plugin_named(&source, "healthy-plugin");
    add_catalog_source(&source);
    std::fs::remove_dir_all(source.join("broken-plugin")).unwrap();

    let inventory =
        inspect_project_workspace_inventory(&claude_installation(), &project_a).unwrap();

    let broken = plugin(&inventory, "broken-plugin");
    assert_eq!(broken.health.status, ResourceHealthStatus::Error);
    assert_eq!(
        broken.health.diagnostic.as_ref().unwrap().code,
        "resource_catalog_binding_invalid"
    );
    assert!(!action_is_available(broken, ResourceAction::Install));
    let healthy = plugin(&inventory, "healthy-plugin");
    assert_eq!(healthy.health.status, ResourceHealthStatus::Healthy);
    assert!(action_is_available(healthy, ResourceAction::Install));
}

#[test]
#[serial(home_env)]
fn missing_managed_plugin_link_is_reported_and_excluded_from_launch() {
    let (_home, _guard, source, project_a, _project_b) = setup();
    write_plugin(&source);
    add_catalog_source(&source);
    let installation_id = claude_installation();
    let inventory = inspect_project_workspace_inventory(&installation_id, &project_a).unwrap();
    apply_action(
        &installation_id,
        &project_a,
        &inventory,
        plugin(&inventory, "native-plugin"),
        ResourceAction::Install,
        &PlanStore::default(),
    );
    let context = AgentContext {
        installation_id: installation_id.clone(),
        project_path: Some(
            std::fs::canonicalize(&project_a)
                .unwrap()
                .to_string_lossy()
                .into_owned(),
        ),
    };
    let link = PathBuf::from(
        &builtin_registry()
            .adapter("claude-code")
            .unwrap()
            .launcher()
            .unwrap()
            .recipe(&context)
            .unwrap()
            .args[1],
    );
    std::fs::remove_file(&link).unwrap();

    let damaged = inspect_project_workspace_inventory(&installation_id, &project_a).unwrap();
    let damaged_plugin = plugin(&damaged, "native-plugin");
    assert_eq!(damaged_plugin.health.status, ResourceHealthStatus::Error);
    assert_eq!(
        damaged_plugin.health.diagnostic.as_ref().unwrap().code,
        "plugin_ownership_invalid"
    );
    assert_eq!(
        damaged_plugin.management.status,
        ResourceManagementStatus::ReadOnly
    );
    assert!(builtin_registry()
        .adapter("claude-code")
        .unwrap()
        .launcher()
        .unwrap()
        .recipe(&context)
        .is_err());
}

#[test]
#[serial(home_env)]
fn plugin_without_claude_descriptor_is_visible_but_not_installable_for_claude() {
    let (_home, _guard, source, project_a, _project_b) = setup();
    let plugin_root = source.join("codex-only");
    std::fs::create_dir_all(plugin_root.join(".codex-plugin")).unwrap();
    std::fs::write(
        plugin_root.join(".codex-plugin/plugin.json"),
        r#"{"name":"codex-only","description":"Codex only"}"#,
    )
    .unwrap();
    add_catalog_source(&source);

    let inventory =
        inspect_project_workspace_inventory(&claude_installation(), &project_a).unwrap();
    let candidate = plugin(&inventory, "codex-only");

    assert_eq!(
        candidate.effective_state,
        EffectiveResourceState::Unconfigured
    );
    assert!(!action_is_available(candidate, ResourceAction::Install));
    assert!(candidate.management.actions.iter().any(|action| {
        action.action == ResourceAction::Install
            && action
                .limitation
                .as_ref()
                .is_some_and(|limitation| limitation.code == "unsupported_agent_capability")
    }));
}

#[test]
#[serial(home_env)]
fn catalog_codex_plugin_installs_toggles_and_removes_inside_the_project_runtime() {
    let home = tempfile::tempdir().unwrap();
    let _guard = EnvironmentGuard::isolated(home.path());
    let codex_home = home.path().join(".codex");
    let source = home.path().join("source");
    let project = home.path().join("project");
    std::fs::create_dir_all(&codex_home).unwrap();
    std::fs::create_dir_all(&project).unwrap();
    let inherited_marketplace = codex_home.join(".tmp/marketplaces/base-market");
    let inherited_package = codex_home.join("plugins/cache/base-market/base-plugin/2.0.0");
    let implicit_package = codex_home.join("plugins/cache/implicit-market/implicit-plugin/1.0.0");
    std::fs::create_dir_all(&inherited_marketplace).unwrap();
    std::fs::create_dir_all(inherited_package.join(".codex-plugin")).unwrap();
    std::fs::create_dir_all(implicit_package.join(".codex-plugin")).unwrap();
    std::fs::write(
        inherited_package.join(".codex-plugin/plugin.json"),
        r#"{"name":"base-plugin","version":"2.0.0"}"#,
    )
    .unwrap();
    std::fs::write(
        implicit_package.join(".codex-plugin/plugin.json"),
        r#"{"name":"implicit-plugin","version":"1.0.0"}"#,
    )
    .unwrap();
    std::fs::write(
        codex_home.join("config.toml"),
        "model = \"test\"\n\n[marketplaces.base-market]\nsource_type = \"git\"\nsource = \"https://example.com/base-market.git\"\n\n[plugins.\"base-plugin@base-market\"]\nenabled = true\n\n[plugins.\"implicit-plugin@implicit-market\"]\nenabled = true\n",
    )
    .unwrap();
    std::fs::write(codex_home.join("auth.json"), "{}\n").unwrap();
    write_codex_plugin_named(&source, "native-plugin", "team", "1.2.3");
    write_codex_plugin_named(&source, "user-plugin", "user-market", "1.0.0");
    let external_skill = home.path().join("external-skill");
    std::fs::create_dir_all(&external_skill).unwrap();
    std::fs::write(
        external_skill.join("SKILL.md"),
        "---\nname: external-skill\n---\n",
    )
    .unwrap();
    std::fs::create_dir_all(source.join("native-plugin/.agents/skills")).unwrap();
    std::os::unix::fs::symlink(
        &external_skill,
        source.join("native-plugin/.agents/skills/external-skill"),
    )
    .unwrap();
    let source_id = add_catalog_source_named(&source, "Codex Plugins");
    let skill_source = home.path().join("skill-source");
    write_skill(&skill_source, "user skill");
    add_catalog_source_named(&skill_source, "Codex Skills");
    let catalog = load_resource_catalog_snapshot().unwrap();
    assert!(catalog
        .resources
        .values()
        .find(|resource| resource.source_id == source_id && resource.install_id == "native-plugin")
        .unwrap()
        .compatible_agents
        .contains("codex"));
    let user_candidate = catalog
        .resources
        .values()
        .find(|resource| resource.source_id == source_id && resource.install_id == "user-plugin")
        .unwrap();
    let user_workspace = resolve_user_agent_workspace(&codex_installation()).unwrap();
    let user_record =
        super::user_plugins::proposed_user_plugin_record(&user_workspace, &user_candidate.id)
            .unwrap();
    super::user_plugins::persist_user_plugin_record_for_test(&user_record).unwrap();
    let user_package = codex_home
        .join("plugins/cache")
        .join(&user_record.marketplace_name)
        .join(&user_record.install_id)
        .join("1.0.0");
    std::fs::create_dir_all(user_package.join(".codex-plugin")).unwrap();
    std::fs::write(
        user_package.join(".codex-plugin/plugin.json"),
        r#"{"name":"user-plugin","version":"1.0.0"}"#,
    )
    .unwrap();
    let base_config_path = codex_home.join("config.toml");
    let mut base_config = std::fs::read_to_string(&base_config_path).unwrap();
    base_config.push_str(&format!(
        "\n[plugins.\"{}\"]\nenabled = true\n",
        user_record.native_id
    ));
    std::fs::write(&base_config_path, base_config).unwrap();
    let catalog_path = home.path().join(".ad/state/resource_catalog.json");
    let mut catalog: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&catalog_path).unwrap()).unwrap();
    for resource in catalog
        .get_mut("resources")
        .and_then(serde_json::Value::as_object_mut)
        .unwrap()
        .values_mut()
    {
        if resource
            .get("installId")
            .and_then(serde_json::Value::as_str)
            == Some("native-plugin")
        {
            resource["compatibleAgents"] = serde_json::json!(["claude-code"]);
        }
    }
    std::fs::write(&catalog_path, serde_json::to_vec_pretty(&catalog).unwrap()).unwrap();
    let installation_id = codex_installation();
    let plans = PlanStore::default();
    let user_inventory = inspect_user_resource_inventory(&installation_id).unwrap();
    apply_user_action(
        &installation_id,
        &user_inventory,
        user_skill(&user_inventory, "review"),
        ResourceAction::Install,
        &PlanStore::default(),
    );
    let base_skill = codex_home.join("skills/review");
    assert!(base_skill.is_symlink());

    let inventory = inspect_project_workspace_inventory(&installation_id, &project).unwrap();
    let candidate = plugin(&inventory, "native-plugin");
    assert!(action_is_available(candidate, ResourceAction::Install));

    apply_action(
        &installation_id,
        &project,
        &inventory,
        candidate,
        ResourceAction::Install,
        &plans,
    );

    let runtime = project_runtime_for_base_project(&installation_id, &project).unwrap();
    let package = runtime
        .runtime_home
        .join("plugins/cache/team/native-plugin/1.2.3");
    assert!(package.join(".codex-plugin/plugin.json").is_file());
    assert!(package.join("skills/demo/SKILL.md").is_file());
    assert!(!package.join(".agents/skills").exists());
    assert!(runtime
        .runtime_home
        .join("plugins/cache/base-market/base-plugin/2.0.0/.codex-plugin/plugin.json")
        .is_file());
    assert!(runtime
        .runtime_home
        .join("plugins/cache/implicit-market/implicit-plugin/1.0.0/.codex-plugin/plugin.json")
        .is_file());
    assert!(runtime
        .runtime_home
        .join(".tmp/marketplaces/base-market")
        .is_dir());
    let config = std::fs::read_to_string(runtime.runtime_home.join("config.toml")).unwrap();
    let config = config.parse::<toml::Value>().unwrap();
    assert_eq!(
        config
            .get("plugins")
            .and_then(|plugins| plugins.get("native-plugin@team"))
            .and_then(|plugin| plugin.get("enabled"))
            .and_then(toml::Value::as_bool),
        Some(true)
    );
    assert_eq!(
        config
            .get("marketplaces")
            .and_then(|marketplaces| marketplaces.get("team"))
            .and_then(|marketplace| marketplace.get("source"))
            .and_then(toml::Value::as_str),
        Some(
            std::fs::canonicalize(source.join("native-plugin"))
                .unwrap()
                .to_string_lossy()
                .as_ref()
        )
    );
    let installations = list_resource_installations().unwrap();
    let plugin_installation = installations
        .iter()
        .find(|installation| installation.adapter_contract == "codex-plugin-store-v1")
        .unwrap();
    assert_eq!(plugin_installation.source_id, source_id);

    let installed = inspect_project_workspace_inventory(&installation_id, &project).unwrap();
    let installed_plugin = plugin(&installed, "native-plugin");
    assert_eq!(
        installed_plugin.effective_state,
        EffectiveResourceState::Enabled
    );
    assert!(action_is_available(
        installed_plugin,
        ResourceAction::Disable
    ));
    assert_eq!(
        installed
            .plugins
            .resources
            .iter()
            .filter(|resource| resource.logical_id == "native-plugin")
            .count(),
        1
    );

    let inherited_skill = skill(&installed, "review");
    assert!(inherited_skill
        .provenance
        .declarations
        .iter()
        .any(|declaration| declaration.scope == Some(ResourceScope::User)));
    assert!(action_is_available(
        inherited_skill,
        ResourceAction::Disable
    ));
    assert!(!action_is_available(
        inherited_skill,
        ResourceAction::Install
    ));
    assert!(!action_is_available(
        inherited_skill,
        ResourceAction::Remove
    ));
    let base_config_before_skill_override = std::fs::read(&base_config_path).unwrap();
    apply_action(
        &installation_id,
        &project,
        &installed,
        inherited_skill,
        ResourceAction::Disable,
        &PlanStore::default(),
    );
    let skill_disabled = inspect_project_workspace_inventory(&installation_id, &project).unwrap();
    assert_eq!(
        skill_disabled
            .skills
            .resources
            .iter()
            .filter(|resource| resource.logical_id == "review")
            .count(),
        1
    );
    assert_eq!(
        skill(&skill_disabled, "review").effective_state,
        EffectiveResourceState::Disabled
    );
    assert!(base_skill.is_symlink());
    assert_eq!(
        std::fs::read(&base_config_path).unwrap(),
        base_config_before_skill_override
    );

    let installed = inspect_project_workspace_inventory(&installation_id, &project).unwrap();
    let inherited_user_plugin = plugin(&installed, "user-plugin");
    assert!(action_is_available(
        inherited_user_plugin,
        ResourceAction::Disable
    ));
    let base_config_before = std::fs::read(&base_config_path).unwrap();
    apply_action(
        &installation_id,
        &project,
        &installed,
        inherited_user_plugin,
        ResourceAction::Disable,
        &PlanStore::default(),
    );
    let user_plugin_disabled =
        inspect_project_workspace_inventory(&installation_id, &project).unwrap();
    assert_eq!(
        user_plugin_disabled
            .plugins
            .resources
            .iter()
            .filter(|resource| resource.logical_id == "user-plugin")
            .count(),
        1
    );
    let inherited_user_plugin = plugin(&user_plugin_disabled, "user-plugin");
    assert_eq!(
        inherited_user_plugin.effective_state,
        EffectiveResourceState::Disabled
    );
    assert_eq!(
        inherited_user_plugin.ownership.kind,
        ResourceOwnershipKind::AdManaged
    );
    assert!(inherited_user_plugin
        .provenance
        .declarations
        .iter()
        .any(|declaration| declaration.scope == Some(ResourceScope::User)));
    assert!(inherited_user_plugin
        .provenance
        .declarations
        .iter()
        .any(|declaration| declaration.scope == Some(ResourceScope::Project)));
    assert_eq!(
        std::fs::read(&base_config_path).unwrap(),
        base_config_before
    );
    assert!(action_is_available(
        inherited_user_plugin,
        ResourceAction::Enable
    ));

    let installed = inspect_project_workspace_inventory(&installation_id, &project).unwrap();
    let installed_plugin = plugin(&installed, "native-plugin");
    apply_action(
        &installation_id,
        &project,
        &installed,
        installed_plugin,
        ResourceAction::Disable,
        &plans,
    );
    let disabled = inspect_project_workspace_inventory(&installation_id, &project).unwrap();
    assert_eq!(
        plugin(&disabled, "native-plugin").effective_state,
        EffectiveResourceState::Disabled
    );

    apply_action(
        &installation_id,
        &project,
        &disabled,
        plugin(&disabled, "native-plugin"),
        ResourceAction::Remove,
        &plans,
    );
    assert!(!package.exists());
    assert!(list_resource_installations()
        .unwrap()
        .iter()
        .all(|installation| installation.adapter_contract != "codex-plugin-store-v1"));
    let removed = inspect_project_workspace_inventory(&installation_id, &project).unwrap();
    assert_eq!(
        plugin(&removed, "native-plugin").effective_state,
        EffectiveResourceState::Unconfigured
    );
}

#[test]
#[serial(home_env)]
fn resource_removal_uninstalls_each_project_before_suppressing_and_readd_does_not_reinstall() {
    let (_home, _guard, source, project_a, project_b) = setup();
    let source_id = add_catalog_source(&source);
    let resource_id = catalog_resource_id(&source_id, ResourceKind::Skills, "review");
    let installation_id = claude_installation();
    let plans = PlanStore::default();
    for project in [&project_a, &project_b] {
        let inventory = inspect_project_workspace_inventory(&installation_id, project).unwrap();
        apply_action(
            &installation_id,
            project,
            &inventory,
            skill(&inventory, "review"),
            ResourceAction::Install,
            &plans,
        );
    }
    let removals = ResourceRemovalPlanStore::default();
    let preview = removals.preview(&resource_id).unwrap();
    assert_eq!(preview.affected_project_count, 2);
    assert_eq!(preview.affected_agent_count, 2);
    let progress = std::sync::Mutex::new(Vec::new());

    let report = removals
        .apply(
            &preview.plan_id,
            &preview.risk_fingerprint,
            true,
            &|event| progress.lock().unwrap().push(event),
        )
        .unwrap();

    assert_eq!(report.phase, ResourceRemovalPhase::Complete, "{report:?}");
    assert_eq!(report.completed, 2);
    assert!(std::fs::symlink_metadata(project_a.join(".claude/skills/review")).is_err());
    assert!(std::fs::symlink_metadata(project_b.join(".claude/skills/review")).is_err());
    assert!(list_resource_installations().unwrap().is_empty());
    assert_eq!(
        load_resource_catalog_snapshot()
            .unwrap()
            .resources
            .get(&resource_id)
            .unwrap()
            .lifecycle,
        ResourceLifecycle::Suppressed
    );
    let events = progress.into_inner().unwrap();
    assert_eq!(events.last().unwrap().phase, ResourceRemovalPhase::Complete);
    assert!(events
        .windows(2)
        .all(|pair| pair[0].sequence < pair[1].sequence));
    let removed_inventory =
        inspect_project_workspace_inventory(&installation_id, &project_a).unwrap();
    assert!(removed_inventory
        .skills
        .resources
        .iter()
        .all(|resource| resource.logical_id != "review"));

    readd_catalog_resource(&resource_id).unwrap();
    assert_eq!(
        load_resource_catalog_snapshot()
            .unwrap()
            .resources
            .get(&resource_id)
            .unwrap()
            .lifecycle,
        ResourceLifecycle::Managed
    );
    assert!(std::fs::symlink_metadata(project_a.join(".claude/skills/review")).is_err());
    assert!(std::fs::symlink_metadata(project_b.join(".claude/skills/review")).is_err());
    let readded_inventory =
        inspect_project_workspace_inventory(&installation_id, &project_a).unwrap();
    assert!(readded_inventory
        .skills
        .resources
        .iter()
        .any(|resource| resource.logical_id == "review"));
}

#[test]
#[serial(home_env)]
fn readd_rejects_a_resource_missing_from_the_live_source() {
    let (_home, _guard, source, project_a, _project_b) = setup();
    let source_id = add_catalog_source(&source);
    let resource_id = catalog_resource_id(&source_id, ResourceKind::Skills, "review");
    let installation_id = claude_installation();
    let inventory = inspect_project_workspace_inventory(&installation_id, &project_a).unwrap();
    apply_action(
        &installation_id,
        &project_a,
        &inventory,
        skill(&inventory, "review"),
        ResourceAction::Install,
        &PlanStore::default(),
    );
    let removals = ResourceRemovalPlanStore::default();
    let preview = removals.preview(&resource_id).unwrap();
    let report = removals
        .apply(&preview.plan_id, &preview.risk_fingerprint, true, &|_| {})
        .unwrap();
    assert_eq!(report.phase, ResourceRemovalPhase::Complete);
    std::fs::remove_dir_all(source.join("review")).unwrap();

    let error = readd_catalog_resource(&resource_id).unwrap_err();

    assert_eq!(error.code, AgentErrorCode::InvalidPlan);
    assert_eq!(
        load_resource_catalog_snapshot()
            .unwrap()
            .resources
            .get(&resource_id)
            .unwrap()
            .lifecycle,
        ResourceLifecycle::Suppressed
    );
}

#[test]
#[serial(home_env)]
fn resource_removal_retry_continues_the_same_durable_operation() {
    let (_home, _guard, source, project_a, project_b) = setup();
    let source_id = add_catalog_source(&source);
    let resource_id = catalog_resource_id(&source_id, ResourceKind::Skills, "review");
    let installation_id = claude_installation();
    let plans = PlanStore::default();
    for project in [&project_a, &project_b] {
        let inventory = inspect_project_workspace_inventory(&installation_id, project).unwrap();
        apply_action(
            &installation_id,
            project,
            &inventory,
            skill(&inventory, "review"),
            ResourceAction::Install,
            &plans,
        );
    }
    let broken_target = project_b.join(".claude/skills/review");
    let original_link = std::fs::read_link(&broken_target).unwrap();
    std::fs::remove_file(&broken_target).unwrap();
    std::os::unix::fs::symlink("../wrong", &broken_target).unwrap();
    let removals = ResourceRemovalPlanStore::default();
    let preview = removals.preview(&resource_id).unwrap();

    let first = removals
        .apply(&preview.plan_id, &preview.risk_fingerprint, true, &|_| {})
        .unwrap();

    assert_eq!(first.phase, ResourceRemovalPhase::PartialFailure);
    assert_eq!(first.completed, 1);
    std::fs::remove_file(&broken_target).unwrap();
    std::os::unix::fs::symlink(original_link, &broken_target).unwrap();

    let retried = removals.retry(&first.operation_id, &|_| {}).unwrap();

    assert_eq!(retried.operation_id, first.operation_id);
    assert_eq!(retried.phase, ResourceRemovalPhase::Complete);
    assert_eq!(retried.completed, 2);
    assert_eq!(retried.total, 2);
    assert!(retried
        .installations
        .iter()
        .all(|item| item.state == ResourceRemovalItemState::Succeeded));
    let persisted = list_resource_removal_operations().unwrap();
    assert_eq!(persisted.len(), 1);
    assert_eq!(persisted[0].operation_id, first.operation_id);
    assert_eq!(persisted[0].phase, ResourceRemovalPhase::Complete);
}

#[test]
#[serial(home_env)]
fn resource_removal_includes_legacy_skill_ownership_without_a_ledger_file() {
    let (_home, _guard, source, project_a, _project_b) = setup();
    let source_id = add_catalog_source(&source);
    let resource_id = catalog_resource_id(&source_id, ResourceKind::Skills, "review");
    let installation_id = claude_installation();
    let inventory = inspect_project_workspace_inventory(&installation_id, &project_a).unwrap();
    apply_action(
        &installation_id,
        &project_a,
        &inventory,
        skill(&inventory, "review"),
        ResourceAction::Install,
        &PlanStore::default(),
    );
    let state = ExecutionState::open().unwrap();
    for name in state.resource_installations().entry_names().unwrap() {
        state
            .resource_installations()
            .remove(name.to_str().unwrap())
            .unwrap();
    }
    for name in state.ownership().entry_names().unwrap() {
        let name = name.to_str().unwrap();
        let mut record = serde_json::from_slice::<ResourceOwnershipRecord>(
            &state.ownership().read(name).unwrap(),
        )
        .unwrap();
        record.schema_version = 2;
        record.catalog_binding = None;
        state
            .ownership()
            .write_atomic(name, &serde_json::to_vec_pretty(&record).unwrap())
            .unwrap();
    }

    let projected =
        super::resource_installations::list_resource_installations_for_lifecycle().unwrap();
    assert_eq!(projected.len(), 1);
    assert_eq!(projected[0].resource_id, resource_id);
    let removals = ResourceRemovalPlanStore::default();
    let preview = removals.preview(&resource_id).unwrap();
    assert_eq!(preview.affected_project_count, 1);

    let report = removals
        .apply(&preview.plan_id, &preview.risk_fingerprint, true, &|_| {})
        .unwrap();

    assert_eq!(report.phase, ResourceRemovalPhase::Complete);
    assert!(std::fs::symlink_metadata(project_a.join(".claude/skills/review")).is_err());
}

#[test]
#[serial(home_env)]
fn resource_lifecycle_lease_blocks_a_concurrent_project_install() {
    let (_home, _guard, source, project_a, _project_b) = setup();
    let source_id = add_catalog_source(&source);
    let resource_id = catalog_resource_id(&source_id, ResourceKind::Skills, "review");
    let installation_id = claude_installation();
    let inventory = inspect_project_workspace_inventory(&installation_id, &project_a).unwrap();
    let resource = skill(&inventory, "review");
    let request = ProjectCollectionActionRequest {
        workspace_key: inventory.workspace.key.clone(),
        inventory_revision: inventory.revision.clone(),
        resource_key: resource.key.clone(),
        action: ResourceAction::Install,
    };
    let plans = PlanStore::default();
    let preview =
        preview_project_collection_action(&installation_id, &project_a, request, &plans).unwrap();
    let state = ExecutionState::open().unwrap();
    let _lease = TargetLockSet::acquire_for_ad_states(
        &[resource_lifecycle_lock_target(&state, &resource_id)],
        "test-removal",
        &state,
    )
    .unwrap();

    let error = apply_project_collection_action_plan(
        &preview.plan.id,
        &preview.plan.context,
        &preview.plan.risk_fingerprint,
        true,
        &plans,
    )
    .unwrap_err();

    assert_eq!(error.code, AgentErrorCode::ResourceChanged);
    assert!(std::fs::symlink_metadata(project_a.join(".claude/skills/review")).is_err());
    assert!(list_resource_installations().unwrap().is_empty());
}

#[test]
#[serial(home_env)]
fn source_removal_composes_resource_uninstall_and_preserves_source_content() {
    let (_home, _guard, source, project_a, project_b) = setup();
    write_plugin(&source);
    let source_id = add_catalog_source(&source);
    let installation_id = claude_installation();
    let plans = PlanStore::default();
    for project in [&project_a, &project_b] {
        let inventory = inspect_project_workspace_inventory(&installation_id, project).unwrap();
        apply_action(
            &installation_id,
            project,
            &inventory,
            skill(&inventory, "review"),
            ResourceAction::Install,
            &plans,
        );
    }
    let inventory = inspect_project_workspace_inventory(&installation_id, &project_a).unwrap();
    apply_action(
        &installation_id,
        &project_a,
        &inventory,
        plugin(&inventory, "native-plugin"),
        ResourceAction::Install,
        &plans,
    );

    let source_removals = SourceRemovalPlanStore::default();
    let preview = source_removals.preview(&source_id).unwrap();
    assert_eq!(preview.resources.len(), 2);
    assert_eq!(preview.affected_project_count, 2);
    assert_eq!(preview.affected_agent_count, 2);
    let progress = std::sync::Mutex::new(Vec::new());
    let report = source_removals
        .apply(
            &preview.plan_id,
            &preview.risk_fingerprint,
            true,
            &ResourceRemovalPlanStore::default(),
            &SkillCatalogPlanStore::default(),
            &|event| progress.lock().unwrap().push(event),
        )
        .unwrap();

    assert_eq!(report.phase, SourceRemovalPhase::Complete);
    assert_eq!(report.completed, 2);
    assert!(list_resource_installations().unwrap().is_empty());
    assert!(load_resource_catalog_snapshot().unwrap().sources.is_empty());
    assert!(source.join("review/SKILL.md").is_file());
    assert!(source
        .join("native-plugin/.claude-plugin/plugin.json")
        .is_file());
    assert!(std::fs::symlink_metadata(project_a.join(".claude/skills/review")).is_err());
    assert!(std::fs::symlink_metadata(project_b.join(".claude/skills/review")).is_err());
    let events = progress.into_inner().unwrap();
    assert_eq!(events.last().unwrap().phase, SourceRemovalPhase::Complete);
    assert!(events
        .windows(2)
        .all(|pair| pair[0].sequence < pair[1].sequence));
}

#[test]
#[serial(home_env)]
fn source_removal_treats_an_absent_uninstalled_resource_as_complete() {
    let (_home, _guard, source, _project_a, _project_b) = setup();
    let keep = source.join("keep");
    std::fs::create_dir_all(&keep).unwrap();
    std::fs::write(keep.join("SKILL.md"), "---\nname: keep\n---\n").unwrap();
    let source_id = add_catalog_source(&source);
    std::fs::remove_dir_all(source.join("review")).unwrap();
    update_catalog_source(&source_id);
    let absent_id = catalog_resource_id(&source_id, ResourceKind::Skills, "review");
    assert!(!load_resource_catalog_snapshot().unwrap().resources[&absent_id].present);

    let source_removals = SourceRemovalPlanStore::default();
    let preview = source_removals.preview(&source_id).unwrap();
    assert_eq!(
        preview
            .resources
            .iter()
            .find(|resource| resource.resource_id == absent_id)
            .unwrap()
            .state,
        ResourceRemovalItemState::Succeeded
    );
    let report = source_removals
        .apply(
            &preview.plan_id,
            &preview.risk_fingerprint,
            true,
            &ResourceRemovalPlanStore::default(),
            &SkillCatalogPlanStore::default(),
            &|_| {},
        )
        .unwrap();

    assert_eq!(report.phase, SourceRemovalPhase::Complete);
    assert!(load_resource_catalog_snapshot().unwrap().sources.is_empty());
}

#[test]
#[serial(home_env)]
fn source_update_blocks_when_an_installed_plugin_disappears() {
    let (_home, _guard, source, project_a, _project_b) = setup();
    write_plugin(&source);
    let source_id = add_catalog_source(&source);
    let installation_id = claude_installation();
    let inventory = inspect_project_workspace_inventory(&installation_id, &project_a).unwrap();
    apply_action(
        &installation_id,
        &project_a,
        &inventory,
        plugin(&inventory, "native-plugin"),
        ResourceAction::Install,
        &PlanStore::default(),
    );
    std::fs::remove_dir_all(source.join("native-plugin")).unwrap();

    let source_plans = SkillCatalogPlanStore::default();
    let update = source_plans.preview_update(&source_id).unwrap();

    assert_eq!(update.applicability, SkillCatalogPlanApplicability::Blocked);
    assert!(update
        .blocking_issues
        .iter()
        .any(|issue| issue.code == "source_update_breaks_installation"));
    assert!(source.join("review/SKILL.md").is_file());
}

#[test]
#[serial(home_env)]
fn source_update_blocks_when_an_installed_plugin_loses_agent_compatibility() {
    let (_home, _guard, source, project_a, _project_b) = setup();
    write_plugin(&source);
    let source_id = add_catalog_source(&source);
    let installation_id = claude_installation();
    let inventory = inspect_project_workspace_inventory(&installation_id, &project_a).unwrap();
    apply_action(
        &installation_id,
        &project_a,
        &inventory,
        plugin(&inventory, "native-plugin"),
        ResourceAction::Install,
        &PlanStore::default(),
    );
    std::fs::remove_dir_all(source.join("native-plugin/.claude-plugin")).unwrap();
    std::fs::create_dir_all(source.join("native-plugin/.codex-plugin")).unwrap();
    std::fs::write(
        source.join("native-plugin/.codex-plugin/plugin.json"),
        r#"{"name":"native-plugin","description":"Codex only"}"#,
    )
    .unwrap();

    let source_plans = SkillCatalogPlanStore::default();
    let update = source_plans.preview_update(&source_id).unwrap();

    assert_eq!(update.applicability, SkillCatalogPlanApplicability::Blocked);
    assert!(update
        .blocking_issues
        .iter()
        .any(|issue| issue.code == "source_update_breaks_installation"));
}

#[test]
#[serial(home_env)]
fn resource_removal_can_uninstall_after_local_source_content_disappears() {
    let (_home, _guard, source, project_a, _project_b) = setup();
    let source_id = add_catalog_source(&source);
    let resource_id = catalog_resource_id(&source_id, ResourceKind::Skills, "review");
    let installation_id = claude_installation();
    let inventory = inspect_project_workspace_inventory(&installation_id, &project_a).unwrap();
    apply_action(
        &installation_id,
        &project_a,
        &inventory,
        skill(&inventory, "review"),
        ResourceAction::Install,
        &PlanStore::default(),
    );
    std::fs::remove_dir_all(source.join("review")).unwrap();

    let removals = ResourceRemovalPlanStore::default();
    let preview = removals.preview(&resource_id).unwrap();
    let report = removals
        .apply(&preview.plan_id, &preview.risk_fingerprint, true, &|_| {})
        .unwrap();

    assert_eq!(report.phase, ResourceRemovalPhase::Complete, "{report:?}");
    assert!(std::fs::symlink_metadata(project_a.join(".claude/skills/review")).is_err());
    assert!(list_resource_installations().unwrap().is_empty());
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
    let inventory = inspect_project_workspace_inventory(&installation_id, &project_a).unwrap();
    let external = skill(&inventory, "external");
    assert_eq!(external.ownership.kind, ResourceOwnershipKind::External);
    assert!(!action_is_available(external, ResourceAction::Remove));
    assert!(external.provenance.source.is_none());
}

#[test]
#[serial(home_env)]
fn same_named_catalog_skills_are_distinct_install_choices() {
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
        .all(|resource| resource.effective_state == EffectiveResourceState::Unconfigured));
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

    let primary = resources
        .iter()
        .find(|resource| resource_source(resource).display_name == "Primary Skills")
        .unwrap();
    let plans = PlanStore::default();
    apply_action(
        &installation_id,
        &project_a,
        &inventory,
        primary,
        ResourceAction::Install,
        &plans,
    );

    let installed = inspect_project_workspace_inventory(&installation_id, &project_a).unwrap();
    let primary = installed
        .skills
        .resources
        .iter()
        .find(|resource| resource_source(resource).display_name == "Primary Skills")
        .unwrap();
    let peer = installed
        .skills
        .resources
        .iter()
        .find(|resource| resource_source(resource).display_name == "Peer Skills")
        .unwrap();
    assert_eq!(primary.effective_state, EffectiveResourceState::Enabled);
    assert_eq!(peer.effective_state, EffectiveResourceState::Unconfigured);
    assert!(!action_is_available(peer, ResourceAction::Install));
    assert!(peer.management.actions.iter().any(|action| {
        action.action == ResourceAction::Install
            && action
                .limitation
                .as_ref()
                .is_some_and(|limitation| limitation.code == "target_occupied")
    }));
}

#[test]
#[serial(home_env)]
fn git_catalog_source_is_redacted_at_the_inventory_boundary() {
    let (home, _guard, source, project_a, _project_b) = setup();
    let source_id = add_catalog_source(&source);
    let catalog_path = home.path().join(".ad/state/resource_catalog.json");
    let mut catalog: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&catalog_path).unwrap()).unwrap();
    let entry = catalog["sources"]
        .as_object_mut()
        .unwrap()
        .get_mut(&source_id)
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
        agent_supported: true,
        ownership_record: None,
        management_record_id: None,
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
fn external_plugin_overrides_are_read_only_in_project_inventory() {
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
    let inventory_a = inspect_project_workspace_inventory(&installation_id, &project_a).unwrap();
    let inventory_b = inspect_project_workspace_inventory(&installation_id, &project_b).unwrap();
    assert_eq!(
        plugin(&inventory_a, "demo").ownership.kind,
        ResourceOwnershipKind::External
    );
    assert!(!action_is_available(
        plugin(&inventory_a, "demo"),
        ResourceAction::Enable
    ));
    assert!(!action_is_available(
        plugin(&inventory_a, "demo"),
        ResourceAction::Disable
    ));
    assert!(!action_is_available(
        plugin(&inventory_a, "demo"),
        ResourceAction::Remove
    ));
    assert!(!action_is_available(
        plugin(&inventory_b, "demo"),
        ResourceAction::Remove
    ));
    assert!(plugin(&inventory_a, "demo").provenance.source.is_none());

    let local_path = project_a.join(".claude/settings.local.json");
    let local: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&local_path).unwrap()).unwrap();
    assert_eq!(local["enabledPlugins"]["demo"], false);
    assert_eq!(local["unknown"], 1);
    assert!(!project_b.join(".claude/settings.local.json").exists());
    let user_before = std::fs::read(home.path().join(".claude/settings.json")).unwrap();

    let local: serde_json::Value =
        serde_json::from_slice(&std::fs::read(local_path).unwrap()).unwrap();
    assert_eq!(local["enabledPlugins"]["demo"], false);
    assert_eq!(local["enabledPlugins"]["keep"], true);
    assert_eq!(local["unknown"], 1);
    assert_eq!(
        std::fs::read(home.path().join(".claude/settings.json")).unwrap(),
        user_before
    );
    assert!(!project_b.join(".claude/settings.local.json").exists());
}

fn user_skill<'a>(inventory: &'a UserResourceInventory, name: &str) -> &'a CollectionResourceView {
    inventory
        .skills
        .resources
        .iter()
        .find(|resource| resource.logical_id == name)
        .unwrap()
}

fn apply_user_action(
    installation_id: &InstallationId,
    inventory: &UserResourceInventory,
    resource: &CollectionResourceView,
    action: ResourceAction,
    plans: &PlanStore,
) {
    let plugin_plans = UserPluginPlanStore::default();
    let preview = preview_user_collection_action(
        installation_id,
        UserCollectionActionRequest {
            workspace_key: inventory.workspace.key.clone(),
            inventory_revision: inventory.revision.clone(),
            resource_key: resource.key.clone(),
            action,
        },
        plans,
        &plugin_plans,
    )
    .unwrap();
    let report = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(apply_user_collection_action_plan(
            &preview.plan.id,
            &preview.plan.context,
            &preview.plan.risk_fingerprint,
            true,
            plans,
            &plugin_plans,
        ))
        .unwrap();
    assert_eq!(report.outcome, WorkspaceOperationOutcome::Changed);
}

#[test]
#[serial(home_env)]
fn claude_user_skill_has_managed_lifecycle_without_removing_on_disable() {
    let (home, _guard, source, _project_a, _project_b) = setup();
    let source_id = add_catalog_source(&source);
    let installation_id = claude_installation();
    let plans = PlanStore::default();

    let available = inspect_user_resource_inventory(&installation_id).unwrap();
    let candidate = user_skill(&available, "review");
    assert_eq!(
        candidate.effective_state,
        EffectiveResourceState::Unconfigured
    );
    apply_user_action(
        &installation_id,
        &available,
        candidate,
        ResourceAction::Install,
        &plans,
    );

    let target = home.path().join(".claude/skills/review");
    assert!(std::fs::symlink_metadata(&target)
        .unwrap()
        .file_type()
        .is_symlink());
    let installed = inspect_user_resource_inventory(&installation_id).unwrap();
    assert_eq!(
        user_skill(&installed, "review").ownership.kind,
        ResourceOwnershipKind::AdManaged
    );
    let installed_artifact = std::fs::read_link(&target).unwrap();
    assert!(std::fs::read_to_string(target.join("SKILL.md"))
        .unwrap()
        .contains("first revision"));
    write_skill(&source, "second revision");
    update_catalog_source(&source_id);
    let outdated = inspect_user_resource_inventory(&installation_id).unwrap();
    assert!(action_is_available(
        user_skill(&outdated, "review"),
        ResourceAction::Update
    ));
    assert_eq!(std::fs::read_link(&target).unwrap(), installed_artifact);
    assert!(std::fs::read_to_string(target.join("SKILL.md"))
        .unwrap()
        .contains("first revision"));
    apply_user_action(
        &installation_id,
        &outdated,
        user_skill(&outdated, "review"),
        ResourceAction::Update,
        &plans,
    );
    assert_ne!(std::fs::read_link(&target).unwrap(), installed_artifact);
    assert!(std::fs::read_to_string(target.join("SKILL.md"))
        .unwrap()
        .contains("second revision"));
    let installed = inspect_user_resource_inventory(&installation_id).unwrap();
    apply_user_action(
        &installation_id,
        &installed,
        user_skill(&installed, "review"),
        ResourceAction::Disable,
        &plans,
    );
    assert!(target.exists());
    let settings: serde_json::Value =
        serde_json::from_slice(&std::fs::read(home.path().join(".claude/settings.json")).unwrap())
            .unwrap();
    assert_eq!(settings["skillOverrides"]["review"], "off");
    let disabled = inspect_user_resource_inventory(&installation_id).unwrap();
    assert_eq!(
        user_skill(&disabled, "review").effective_state,
        EffectiveResourceState::Disabled
    );

    apply_user_action(
        &installation_id,
        &disabled,
        user_skill(&disabled, "review"),
        ResourceAction::Remove,
        &plans,
    );
    assert!(std::fs::symlink_metadata(target).is_err());
    let settings: serde_json::Value =
        serde_json::from_slice(&std::fs::read(home.path().join(".claude/settings.json")).unwrap())
            .unwrap();
    assert!(settings
        .get("skillOverrides")
        .and_then(serde_json::Value::as_object)
        .is_none_or(|overrides| !overrides.contains_key("review")));
    let removed = inspect_user_resource_inventory(&installation_id).unwrap();
    assert_eq!(
        user_skill(&removed, "review").effective_state,
        EffectiveResourceState::Unconfigured
    );
    apply_user_action(
        &installation_id,
        &removed,
        user_skill(&removed, "review"),
        ResourceAction::Install,
        &plans,
    );
    let reinstalled = inspect_user_resource_inventory(&installation_id).unwrap();
    assert_eq!(
        user_skill(&reinstalled, "review").effective_state,
        EffectiveResourceState::Enabled
    );
}

#[test]
#[serial(home_env)]
fn user_skill_target_without_ad_ownership_is_external_and_read_only() {
    let (home, _guard, source, project_a, _project_b) = setup();
    add_catalog_source(&source);
    let target = home.path().join(".claude/skills/review");
    std::fs::create_dir_all(target.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(source.join("review"), &target).unwrap();

    let inventory = inspect_user_resource_inventory(&claude_installation()).unwrap();
    let resource = user_skill(&inventory, "review");
    assert_eq!(resource.ownership.kind, ResourceOwnershipKind::External);
    for action in [
        ResourceAction::Install,
        ResourceAction::Update,
        ResourceAction::Remove,
        ResourceAction::Enable,
        ResourceAction::Disable,
    ] {
        assert!(!action_is_available(resource, action));
    }

    let project_inventory =
        inspect_project_workspace_inventory(&claude_installation(), &project_a).unwrap();
    let inherited = project_inventory
        .skills
        .resources
        .iter()
        .find(|resource| {
            resource.logical_id == "review"
                && resource.ownership.kind == ResourceOwnershipKind::External
        })
        .unwrap();
    assert_eq!(inherited.ownership.kind, ResourceOwnershipKind::External);
    for action in [
        ResourceAction::Enable,
        ResourceAction::Disable,
        ResourceAction::Update,
        ResourceAction::Remove,
    ] {
        assert!(!action_is_available(inherited, action));
    }
}

#[test]
#[serial(home_env)]
fn native_user_plugin_without_ad_record_is_external_and_read_only() {
    let (home, _guard, source, _project_a, _project_b) = setup();
    write_plugin(&source);
    add_catalog_source(&source);
    let installation_id = claude_installation();
    std::fs::write(
        home.path().join(".claude/settings.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "enabledPlugins": {"native-plugin@other-market": true}
        }))
        .unwrap(),
    )
    .unwrap();

    let inventory = inspect_user_resource_inventory(&installation_id).unwrap();
    let user_plugin = inventory
        .plugins
        .resources
        .iter()
        .find(|resource| resource.logical_id == "native-plugin")
        .unwrap();
    assert_eq!(
        inventory
            .plugins
            .resources
            .iter()
            .filter(|resource| resource.logical_id == "native-plugin")
            .count(),
        1
    );
    assert_eq!(user_plugin.ownership.kind, ResourceOwnershipKind::External);
    for action in [
        ResourceAction::Install,
        ResourceAction::Update,
        ResourceAction::Remove,
        ResourceAction::Enable,
        ResourceAction::Disable,
    ] {
        assert!(!action_is_available(user_plugin, action));
    }
}

#[test]
#[serial(home_env)]
fn user_plugin_install_preview_is_user_scoped_and_does_not_run_the_cli() {
    let (home, _guard, source, _project_a, _project_b) = setup();
    write_plugin(&source);
    add_catalog_source(&source);
    let installation_id = claude_installation();
    let inventory = inspect_user_resource_inventory(&installation_id).unwrap();
    let candidate = inventory
        .plugins
        .resources
        .iter()
        .find(|resource| resource.logical_id == "native-plugin")
        .unwrap();
    assert!(action_is_available(candidate, ResourceAction::Install));
    let preview = preview_user_collection_action(
        &installation_id,
        UserCollectionActionRequest {
            workspace_key: inventory.workspace.key.clone(),
            inventory_revision: inventory.revision.clone(),
            resource_key: candidate.key.clone(),
            action: ResourceAction::Install,
        },
        &PlanStore::default(),
        &UserPluginPlanStore::default(),
    )
    .unwrap();

    assert_eq!(preview.plan.context.project_path, None);
    assert_eq!(preview.plan.changes[0].scope, ResourceScope::User);
    assert_eq!(
        preview.plan.required_acknowledgements[0].code,
        PlanAcknowledgementCode::UserCollectionApply
    );
    assert!(home
        .path()
        .join(".ad/state/user-plugin-marketplaces")
        .read_dir()
        .unwrap()
        .next()
        .is_none());
}

#[test]
#[serial(home_env)]
fn ad_recorded_native_user_plugin_exposes_managed_lifecycle() {
    let (home, _guard, source, project_a, _project_b) = setup();
    write_plugin(&source);
    add_catalog_source(&source);
    let installation_id = claude_installation();
    let workspace = resolve_user_agent_workspace(&installation_id).unwrap();
    let catalog = load_resource_catalog_snapshot().unwrap();
    let candidate = catalog
        .resources
        .values()
        .find(|resource| {
            resource.kind == ResourceKind::Plugins && resource.install_id == "native-plugin"
        })
        .unwrap();
    let record =
        super::user_plugins::proposed_user_plugin_record(&workspace, &candidate.id).unwrap();
    super::user_plugins::persist_user_plugin_record_for_test(&record).unwrap();
    std::fs::write(
        home.path().join(".claude/settings.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "enabledPlugins": {record.native_id: true}
        }))
        .unwrap(),
    )
    .unwrap();

    let inventory = inspect_user_resource_inventory(&installation_id).unwrap();
    let user_plugin = inventory
        .plugins
        .resources
        .iter()
        .find(|resource| resource.logical_id == "native-plugin")
        .unwrap();
    assert_eq!(user_plugin.ownership.kind, ResourceOwnershipKind::AdManaged);
    assert!(user_plugin.ownership.record_id.is_some());
    for action in [ResourceAction::Disable, ResourceAction::Remove] {
        assert!(action_is_available(user_plugin, action));
    }
    assert!(!action_is_available(user_plugin, ResourceAction::Update));

    let project_inventory =
        inspect_project_workspace_inventory(&installation_id, &project_a).unwrap();
    let inherited = plugin(&project_inventory, "native-plugin");
    assert_eq!(
        project_inventory
            .plugins
            .resources
            .iter()
            .filter(|resource| resource.logical_id == "native-plugin")
            .count(),
        1
    );
    assert!(inherited
        .provenance
        .declarations
        .iter()
        .any(|declaration| declaration.scope == Some(ResourceScope::User)));
    assert!(action_is_available(inherited, ResourceAction::Disable));
    assert!(!action_is_available(inherited, ResourceAction::Remove));

    let user_settings_before = std::fs::read(home.path().join(".claude/settings.json")).unwrap();
    apply_action(
        &installation_id,
        &project_a,
        &project_inventory,
        inherited,
        ResourceAction::Disable,
        &PlanStore::default(),
    );
    let disabled = inspect_project_workspace_inventory(&installation_id, &project_a).unwrap();
    assert_eq!(
        disabled
            .plugins
            .resources
            .iter()
            .filter(|resource| resource.logical_id == "native-plugin")
            .count(),
        1
    );
    let disabled_plugin = plugin(&disabled, "native-plugin");
    assert_eq!(
        disabled_plugin.effective_state,
        EffectiveResourceState::Disabled
    );
    assert_eq!(
        disabled_plugin.ownership.kind,
        ResourceOwnershipKind::AdManaged
    );
    assert!(disabled_plugin
        .provenance
        .declarations
        .iter()
        .any(|declaration| declaration.scope == Some(ResourceScope::User)));
    assert!(disabled_plugin
        .provenance
        .declarations
        .iter()
        .any(|declaration| declaration.scope == Some(ResourceScope::Project)));
    assert!(action_is_available(disabled_plugin, ResourceAction::Enable));
    assert_eq!(
        std::fs::read(home.path().join(".claude/settings.json")).unwrap(),
        user_settings_before
    );
    apply_action(
        &installation_id,
        &project_a,
        &disabled,
        disabled_plugin,
        ResourceAction::Enable,
        &PlanStore::default(),
    );
    let enabled = inspect_project_workspace_inventory(&installation_id, &project_a).unwrap();
    assert_eq!(
        enabled
            .plugins
            .resources
            .iter()
            .filter(|resource| resource.logical_id == "native-plugin")
            .count(),
        1
    );
    assert_eq!(
        plugin(&enabled, "native-plugin").effective_state,
        EffectiveResourceState::Enabled
    );
    let removal_error = ResourceRemovalPlanStore::default()
        .preview(&candidate.id)
        .unwrap_err();
    assert!(removal_error.message.contains("user-level Plugin"));
    let update = SkillCatalogPlanStore::default()
        .preview_update(&record.source_id)
        .unwrap();
    assert_eq!(update.applicability, SkillCatalogPlanApplicability::Blocked);
    assert!(update
        .blocking_issues
        .iter()
        .any(|issue| { issue.code == "source_update_requires_user_plugin_removal" }));
}

#[test]
#[serial(home_env)]
fn project_inherits_user_skill_and_can_disable_only_its_local_view() {
    let (home, _guard, source, project_a, project_b) = setup();
    add_catalog_source(&source);
    let installation_id = claude_installation();
    let user_plans = PlanStore::default();
    let available = inspect_user_resource_inventory(&installation_id).unwrap();
    apply_user_action(
        &installation_id,
        &available,
        user_skill(&available, "review"),
        ResourceAction::Install,
        &user_plans,
    );

    let project_inventory =
        inspect_project_workspace_inventory(&installation_id, &project_a).unwrap();
    assert_eq!(
        project_inventory
            .skills
            .resources
            .iter()
            .filter(|resource| resource.logical_id == "review")
            .count(),
        1
    );
    let inherited = skill(&project_inventory, "review");
    assert!(inherited
        .provenance
        .declarations
        .iter()
        .any(|declaration| declaration.scope == Some(ResourceScope::User)));
    assert!(action_is_available(inherited, ResourceAction::Disable));
    assert!(!action_is_available(inherited, ResourceAction::Install));
    assert!(!action_is_available(inherited, ResourceAction::Remove));

    apply_action(
        &installation_id,
        &project_a,
        &project_inventory,
        inherited,
        ResourceAction::Disable,
        &PlanStore::default(),
    );
    let local: serde_json::Value = serde_json::from_slice(
        &std::fs::read(project_a.join(".claude/settings.local.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(local["skillOverrides"]["review"], "off");
    assert!(home.path().join(".claude/skills/review").exists());
    assert!(!project_b.join(".claude/settings.local.json").exists());
    assert_eq!(
        user_skill(
            &inspect_user_resource_inventory(&installation_id).unwrap(),
            "review"
        )
        .effective_state,
        EffectiveResourceState::Enabled
    );
}

#[test]
#[serial(home_env)]
fn existing_project_skill_remains_the_override_after_user_install() {
    let (home, _guard, source, project_a, _project_b) = setup();
    add_catalog_source(&source);
    let installation_id = claude_installation();
    let project_available =
        inspect_project_workspace_inventory(&installation_id, &project_a).unwrap();
    apply_action(
        &installation_id,
        &project_a,
        &project_available,
        skill(&project_available, "review"),
        ResourceAction::Install,
        &PlanStore::default(),
    );
    let user_available = inspect_user_resource_inventory(&installation_id).unwrap();
    apply_user_action(
        &installation_id,
        &user_available,
        user_skill(&user_available, "review"),
        ResourceAction::Install,
        &PlanStore::default(),
    );

    let inventory = inspect_project_workspace_inventory(&installation_id, &project_a).unwrap();
    let resource = skill(&inventory, "review");
    assert_eq!(
        resource
            .provenance
            .declarations
            .last()
            .and_then(|declaration| declaration.scope),
        Some(ResourceScope::Project)
    );
    assert!(action_is_available(resource, ResourceAction::Remove));

    apply_action(
        &installation_id,
        &project_a,
        &inventory,
        resource,
        ResourceAction::Disable,
        &PlanStore::default(),
    );
    let disabled = inspect_project_workspace_inventory(&installation_id, &project_a).unwrap();
    apply_action(
        &installation_id,
        &project_a,
        &disabled,
        skill(&disabled, "review"),
        ResourceAction::Remove,
        &PlanStore::default(),
    );
    assert!(std::fs::symlink_metadata(project_a.join(".claude/skills/review")).is_err());
    assert!(home.path().join(".claude/skills/review").exists());
    let inherited = inspect_project_workspace_inventory(&installation_id, &project_a).unwrap();
    let inherited = skill(&inherited, "review");
    assert_eq!(inherited.effective_state, EffectiveResourceState::Enabled);
    assert_eq!(
        inherited
            .provenance
            .declarations
            .last()
            .and_then(|declaration| declaration.scope),
        Some(ResourceScope::User)
    );
    let local: serde_json::Value = serde_json::from_slice(
        &std::fs::read(project_a.join(".claude/settings.local.json")).unwrap(),
    )
    .unwrap();
    assert!(local
        .get("skillOverrides")
        .and_then(serde_json::Value::as_object)
        .is_none_or(|overrides| !overrides.contains_key("review")));
}

#[test]
#[serial(home_env)]
fn same_named_user_skills_keep_ownership_bound_to_their_catalog_source() {
    let (home, _guard, source, _project_a, _project_b) = setup();
    let peer_source = home.path().join("peer-source");
    write_skill(&peer_source, "peer revision");
    add_catalog_source_named(&source, "Primary Skills");
    add_catalog_source_named(&peer_source, "Peer Skills");
    let installation_id = claude_installation();
    let available = inspect_user_resource_inventory(&installation_id).unwrap();
    let primary = available
        .skills
        .resources
        .iter()
        .find(|resource| resource_source(resource).display_name == "Primary Skills")
        .unwrap();
    apply_user_action(
        &installation_id,
        &available,
        primary,
        ResourceAction::Install,
        &PlanStore::default(),
    );

    let installed = inspect_user_resource_inventory(&installation_id).unwrap();
    let primary = installed
        .skills
        .resources
        .iter()
        .find(|resource| resource_source(resource).display_name == "Primary Skills")
        .unwrap();
    let peer = installed
        .skills
        .resources
        .iter()
        .find(|resource| resource_source(resource).display_name == "Peer Skills")
        .unwrap();
    assert_eq!(primary.ownership.kind, ResourceOwnershipKind::AdManaged);
    assert_eq!(peer.ownership.kind, ResourceOwnershipKind::External);
    assert!(primary.ownership.record_id.is_some());
    assert!(peer.ownership.record_id.is_none());
    assert!(action_is_available(primary, ResourceAction::Remove));
    assert!(!action_is_available(peer, ResourceAction::Update));
    assert!(!action_is_available(peer, ResourceAction::Remove));
}

#[test]
#[serial(home_env)]
fn claude_user_source_batch_merges_shared_skill_override_settings() {
    let (home, _guard, source, _project_a, _project_b) = setup();
    write_skill_named(&source, "format", "Format code", "format revision");
    add_catalog_source(&source);
    let installation_id = claude_installation();
    let inventory = inspect_user_resource_inventory(&installation_id).unwrap();
    let review = user_skill(&inventory, "review");
    let plans = PlanStore::default();
    let preview = preview_user_collection_source_install(
        &installation_id,
        UserCollectionSourceInstallRequest {
            workspace_key: inventory.workspace.key.clone(),
            inventory_revision: inventory.revision.clone(),
            source_resource_key: review.key.clone(),
        },
        &plans,
    )
    .unwrap();
    assert_eq!(preview.resource_keys.len(), 2);
    assert_eq!(
        preview
            .plan
            .changes
            .iter()
            .filter(|change| change.resource.kind == ResourceKind::Settings)
            .count(),
        1
    );
    let report = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(apply_user_collection_action_plan(
            &preview.plan.id,
            &preview.plan.context,
            &preview.plan.risk_fingerprint,
            true,
            &plans,
            &UserPluginPlanStore::default(),
        ))
        .unwrap();
    assert_eq!(report.outcome, WorkspaceOperationOutcome::Changed);
    assert!(home.path().join(".claude/skills/review").exists());
    assert!(home.path().join(".claude/skills/format").exists());
    let settings: serde_json::Value =
        serde_json::from_slice(&std::fs::read(home.path().join(".claude/settings.json")).unwrap())
            .unwrap();
    assert_eq!(settings["skillOverrides"]["review"], "on");
    assert_eq!(settings["skillOverrides"]["format"], "on");
}

#[test]
#[serial(home_env)]
fn codex_user_skill_installs_and_toggles_in_user_scope() {
    let (home, _guard, source, _project_a, _project_b) = setup();
    std::fs::create_dir_all(home.path().join(".codex")).unwrap();
    add_catalog_source(&source);
    let installation_id = codex_installation();
    let available = inspect_user_resource_inventory(&installation_id).unwrap();
    apply_user_action(
        &installation_id,
        &available,
        user_skill(&available, "review"),
        ResourceAction::Install,
        &PlanStore::default(),
    );
    let target = home.path().join(".codex/skills/review");
    assert!(target.is_symlink());

    let installed = inspect_user_resource_inventory(&installation_id).unwrap();
    apply_user_action(
        &installation_id,
        &installed,
        user_skill(&installed, "review"),
        ResourceAction::Disable,
        &PlanStore::default(),
    );
    let disabled = inspect_user_resource_inventory(&installation_id).unwrap();
    assert_eq!(
        user_skill(&disabled, "review").effective_state,
        EffectiveResourceState::Disabled
    );
    assert!(target.is_symlink());
}

#[test]
#[serial(home_env)]
fn codex_managed_user_plugin_toggle_uses_native_user_config() {
    let (home, _guard, source, _project_a, _project_b) = setup();
    std::fs::create_dir_all(home.path().join(".codex")).unwrap();
    write_codex_plugin_named(&source, "native-plugin", "source-market", "1.0.0");
    add_catalog_source(&source);
    let installation_id = codex_installation();
    let workspace = resolve_user_agent_workspace(&installation_id).unwrap();
    let catalog = load_resource_catalog_snapshot().unwrap();
    let candidate = catalog
        .resources
        .values()
        .find(|resource| {
            resource.kind == ResourceKind::Plugins && resource.install_id == "native-plugin"
        })
        .unwrap();
    let record =
        super::user_plugins::proposed_user_plugin_record(&workspace, &candidate.id).unwrap();
    super::user_plugins::persist_user_plugin_record_for_test(&record).unwrap();
    std::fs::write(
        home.path().join(".codex/config.toml"),
        format!("[plugins.\"{}\"]\nenabled = true\n", record.native_id),
    )
    .unwrap();

    let inventory = inspect_user_resource_inventory(&installation_id).unwrap();
    let plugin = inventory
        .plugins
        .resources
        .iter()
        .find(|resource| resource.logical_id == "native-plugin")
        .unwrap();
    assert_eq!(
        inventory
            .plugins
            .resources
            .iter()
            .filter(|resource| resource.logical_id == "native-plugin")
            .count(),
        1
    );
    assert_eq!(plugin.ownership.kind, ResourceOwnershipKind::AdManaged);
    apply_user_action(
        &installation_id,
        &inventory,
        plugin,
        ResourceAction::Disable,
        &PlanStore::default(),
    );
    let config = std::fs::read_to_string(home.path().join(".codex/config.toml"))
        .unwrap()
        .parse::<toml::Value>()
        .unwrap();
    assert_eq!(
        config["plugins"][&record.native_id]["enabled"].as_bool(),
        Some(false)
    );
}

#[test]
#[serial(home_env)]
fn codex_external_same_name_plugin_in_another_marketplace_blocks_install() {
    let (home, _guard, source, _project_a, _project_b) = setup();
    std::fs::create_dir_all(home.path().join(".codex")).unwrap();
    write_codex_plugin_named(&source, "native-plugin", "source-market", "1.0.0");
    add_catalog_source(&source);
    std::fs::write(
        home.path().join(".codex/config.toml"),
        "[plugins.\"native-plugin@other-market\"]\nenabled = true\n",
    )
    .unwrap();

    let inventory = inspect_user_resource_inventory(&codex_installation()).unwrap();
    let plugin = inventory
        .plugins
        .resources
        .iter()
        .find(|resource| resource.logical_id == "native-plugin")
        .unwrap();
    assert_eq!(plugin.ownership.kind, ResourceOwnershipKind::External);
    assert!(!action_is_available(plugin, ResourceAction::Install));
    assert!(!action_is_available(plugin, ResourceAction::Remove));
}

#[test]
#[serial(home_env)]
fn codex_project_inherited_toggles_require_a_prepared_runtime() {
    let (home, _guard, source, project_a, _project_b) = setup();
    std::fs::create_dir_all(home.path().join(".codex")).unwrap();
    std::fs::write(
        home.path().join(".codex/config.toml"),
        "[plugins.\"external@market\"]\nenabled = true\n",
    )
    .unwrap();
    add_catalog_source(&source);
    let installation_id = codex_installation();
    let user = inspect_user_resource_inventory(&installation_id).unwrap();
    apply_user_action(
        &installation_id,
        &user,
        user_skill(&user, "review"),
        ResourceAction::Install,
        &PlanStore::default(),
    );
    let user_config_before = std::fs::read(home.path().join(".codex/config.toml")).unwrap();

    let project = inspect_project_workspace_inventory(&installation_id, &project_a).unwrap();
    let inherited_skill = skill(&project, "review");
    assert!(!action_is_available(
        inherited_skill,
        ResourceAction::Disable
    ));
    assert!(inherited_skill.management.actions.iter().any(|action| {
        action.action == ResourceAction::Disable
            && action
                .limitation
                .as_ref()
                .is_some_and(|limitation| limitation.code == "codex_runtime_not_prepared")
    }));
    let external_plugin = plugin(&project, "external@market");
    assert_eq!(
        external_plugin.ownership.kind,
        ResourceOwnershipKind::External
    );
    assert!(!action_is_available(
        external_plugin,
        ResourceAction::Disable
    ));
    assert!(!action_is_available(
        external_plugin,
        ResourceAction::Enable
    ));
    assert_eq!(
        std::fs::read(home.path().join(".codex/config.toml")).unwrap(),
        user_config_before
    );
}
