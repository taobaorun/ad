use tauri::{ipc::Channel, State};

use crate::agents::{
    apply_legacy_project_skill_migration, apply_skill_catalog_plan, inspect_legacy_skill_inventory,
    load_resource_catalog_snapshot, load_skill_catalog_snapshot,
    preview_legacy_project_skill_migration as preview_legacy_project_skill_migration_backend,
    preview_rollback_skill_catalog_source, public_resource_catalog_snapshot,
    rollback_legacy_project_skill_migration, LegacySkillInventory, LegacySkillMigrationPlanClaim,
    LegacySkillMigrationPlanStore, LegacySkillMigrationPlanView, LegacySkillMigrationReport,
    PlanId, ReceiptId, ResourceCatalogSnapshot, ResourceRemovalOperationSnapshot,
    ResourceRemovalPlanStore, ResourceRemovalPlanView, ResourceRemovalProgress,
    ResourceRemovalReport, RiskFingerprint, SkillCatalogOperationReport, SkillCatalogPlanClaim,
    SkillCatalogPlanStore, SkillCatalogPlanView, SkillCatalogSnapshot, SkillSourcePreviewProgress,
    SkillSourceRequest, SourceRemovalPlanStore, SourceRemovalPlanView, SourceRemovalProgress,
    SourceRemovalReport,
};

use super::{CmdResult, CommandError};

#[tauri::command]
pub fn list_skill_catalog() -> CmdResult<SkillCatalogSnapshot> {
    load_skill_catalog_snapshot().map_err(command_error)
}

#[tauri::command]
pub fn list_resource_catalog() -> CmdResult<ResourceCatalogSnapshot> {
    load_resource_catalog_snapshot()
        .map(public_resource_catalog_snapshot)
        .map_err(command_error)
}

#[tauri::command]
pub fn preview_remove_catalog_resource(
    resource_id: String,
    plans: State<'_, ResourceRemovalPlanStore>,
) -> CmdResult<ResourceRemovalPlanView> {
    plans.preview(&resource_id).map_err(command_error)
}

#[tauri::command]
pub fn apply_remove_catalog_resource(
    plan_id: PlanId,
    risk_fingerprint: RiskFingerprint,
    confirmed: bool,
    on_progress: Channel<ResourceRemovalProgress>,
    plans: State<'_, ResourceRemovalPlanStore>,
) -> CmdResult<ResourceRemovalReport> {
    plans
        .apply(&plan_id, &risk_fingerprint, confirmed, &|progress| {
            let _ = on_progress.send(progress);
        })
        .map_err(command_error)
}

#[tauri::command]
pub fn list_resource_removal_operations() -> CmdResult<Vec<ResourceRemovalOperationSnapshot>> {
    crate::agents::list_resource_removal_operations().map_err(command_error)
}

#[tauri::command]
pub fn retry_remove_catalog_resource(
    operation_id: String,
    on_progress: Channel<ResourceRemovalProgress>,
    plans: State<'_, ResourceRemovalPlanStore>,
) -> CmdResult<ResourceRemovalReport> {
    plans
        .retry(&operation_id, &|progress| {
            let _ = on_progress.send(progress);
        })
        .map_err(command_error)
}

#[tauri::command]
pub fn readd_catalog_resource(resource_id: String) -> CmdResult<ResourceCatalogSnapshot> {
    crate::agents::readd_catalog_resource(&resource_id)
        .map(public_resource_catalog_snapshot)
        .map_err(command_error)
}

#[tauri::command]
pub fn preview_remove_catalog_source(
    source_id: String,
    plans: State<'_, SourceRemovalPlanStore>,
) -> CmdResult<SourceRemovalPlanView> {
    plans.preview(&source_id).map_err(command_error)
}

#[tauri::command]
pub fn apply_remove_catalog_source(
    plan_id: PlanId,
    risk_fingerprint: RiskFingerprint,
    confirmed: bool,
    on_progress: Channel<SourceRemovalProgress>,
    plans: State<'_, SourceRemovalPlanStore>,
    resource_plans: State<'_, ResourceRemovalPlanStore>,
    source_plans: State<'_, SkillCatalogPlanStore>,
) -> CmdResult<SourceRemovalReport> {
    plans
        .apply(
            &plan_id,
            &risk_fingerprint,
            confirmed,
            &resource_plans,
            &source_plans,
            &|progress| {
                let _ = on_progress.send(progress);
            },
        )
        .map_err(command_error)
}

#[tauri::command(async)]
pub fn preview_add_skill_catalog_source(
    request: SkillSourceRequest,
    on_progress: Channel<SkillSourcePreviewProgress>,
    plans: State<'_, SkillCatalogPlanStore>,
) -> CmdResult<SkillCatalogPlanView> {
    plans
        .preview_add_with_progress(request, &|progress| {
            let _ = on_progress.send(progress);
        })
        .map_err(command_error)
}

#[tauri::command(async)]
pub fn preview_update_skill_catalog_source(
    source_id: String,
    plans: State<'_, SkillCatalogPlanStore>,
) -> CmdResult<SkillCatalogPlanView> {
    plans.preview_update(&source_id).map_err(command_error)
}

#[tauri::command]
pub fn preview_remove_skill_catalog_source(
    _source_id: String,
    _plans: State<'_, SkillCatalogPlanStore>,
) -> CmdResult<SkillCatalogPlanView> {
    Err(CommandError::Generic(
        "Source removal must use the Resource Center lifecycle so affected projects are uninstalled first"
            .into(),
    ))
}

#[tauri::command]
pub fn preview_rollback_skill_catalog_source_update(
    receipt_id: ReceiptId,
    plans: State<'_, SkillCatalogPlanStore>,
) -> CmdResult<SkillCatalogPlanView> {
    preview_rollback_skill_catalog_source(&receipt_id, &plans).map_err(command_error)
}

#[tauri::command(async)]
pub fn apply_skill_catalog_source_plan(
    claim: SkillCatalogPlanClaim,
    plans: State<'_, SkillCatalogPlanStore>,
) -> CmdResult<SkillCatalogOperationReport> {
    apply_skill_catalog_plan(&plans, &claim).map_err(command_error)
}

#[tauri::command]
pub fn cancel_skill_catalog_source_plan(
    plan_id: PlanId,
    plans: State<'_, SkillCatalogPlanStore>,
) -> CmdResult<bool> {
    plans.cancel(&plan_id).map_err(command_error)
}

#[tauri::command]
pub fn inspect_legacy_skill_state() -> CmdResult<LegacySkillInventory> {
    inspect_legacy_skill_inventory().map_err(command_error)
}

#[tauri::command]
pub fn preview_legacy_project_skill_migration(
    project_path: String,
    plans: State<'_, LegacySkillMigrationPlanStore>,
) -> CmdResult<LegacySkillMigrationPlanView> {
    preview_legacy_project_skill_migration_backend(std::path::Path::new(&project_path), &plans)
        .map_err(command_error)
}

#[tauri::command]
pub fn apply_legacy_project_skill_migration_plan(
    claim: LegacySkillMigrationPlanClaim,
    plans: State<'_, LegacySkillMigrationPlanStore>,
) -> CmdResult<LegacySkillMigrationReport> {
    apply_legacy_project_skill_migration(&plans, &claim).map_err(command_error)
}

#[tauri::command]
pub fn cancel_legacy_project_skill_migration_plan(
    plan_id: PlanId,
    plans: State<'_, LegacySkillMigrationPlanStore>,
) -> CmdResult<bool> {
    plans.cancel(&plan_id).map_err(command_error)
}

#[tauri::command]
pub fn restore_legacy_project_skill_state(
    receipt_id: ReceiptId,
) -> CmdResult<LegacySkillMigrationReport> {
    rollback_legacy_project_skill_migration(&receipt_id).map_err(command_error)
}

fn command_error(error: impl std::fmt::Display) -> CommandError {
    CommandError::Generic(error.to_string())
}
