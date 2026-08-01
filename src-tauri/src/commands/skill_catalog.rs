use tauri::State;

use crate::agents::{
    apply_legacy_project_skill_migration, apply_skill_catalog_plan, inspect_legacy_skill_inventory,
    load_skill_catalog_snapshot,
    preview_legacy_project_skill_migration as preview_legacy_project_skill_migration_backend,
    rollback_legacy_project_skill_migration, LegacySkillInventory, LegacySkillMigrationPlanClaim,
    LegacySkillMigrationPlanStore, LegacySkillMigrationPlanView, LegacySkillMigrationReport,
    PlanId, ReceiptId, SkillCatalogOperationReport, SkillCatalogPlanClaim, SkillCatalogPlanStore,
    SkillCatalogPlanView, SkillCatalogSnapshot, SkillSourceRequest,
};

use super::{CmdResult, CommandError};

#[tauri::command]
pub fn list_skill_catalog() -> CmdResult<SkillCatalogSnapshot> {
    load_skill_catalog_snapshot().map_err(command_error)
}

#[tauri::command]
pub fn preview_add_skill_catalog_source(
    request: SkillSourceRequest,
    plans: State<'_, SkillCatalogPlanStore>,
) -> CmdResult<SkillCatalogPlanView> {
    plans.preview_add(request).map_err(command_error)
}

#[tauri::command]
pub fn preview_update_skill_catalog_source(
    source_id: String,
    plans: State<'_, SkillCatalogPlanStore>,
) -> CmdResult<SkillCatalogPlanView> {
    plans.preview_update(&source_id).map_err(command_error)
}

#[tauri::command]
pub fn preview_remove_skill_catalog_source(
    source_id: String,
    plans: State<'_, SkillCatalogPlanStore>,
) -> CmdResult<SkillCatalogPlanView> {
    plans.preview_remove(&source_id).map_err(command_error)
}

#[tauri::command]
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
