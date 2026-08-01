use tauri::State;

use crate::agents::{
    apply_skill_catalog_plan, inspect_legacy_skill_inventory, load_skill_catalog_snapshot,
    LegacySkillInventory, SkillCatalogOperationReport, SkillCatalogPlanClaim,
    SkillCatalogPlanStore, SkillCatalogPlanView, SkillCatalogSnapshot, SkillSourceRequest,
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
pub fn inspect_legacy_skill_state() -> CmdResult<LegacySkillInventory> {
    inspect_legacy_skill_inventory().map_err(command_error)
}

fn command_error(error: impl std::fmt::Display) -> CommandError {
    CommandError::Generic(error.to_string())
}
