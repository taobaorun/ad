use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use super::execution_state::ExecutionState;
use super::{
    load_ownership_record_by_id, load_resource_catalog_snapshot, opaque_contract_id, AgentId,
    InstallationId, OwnershipRecordId, PhysicalTargetId, ReceiptId, ResourceInstallationId,
    ResourceKind, ResourceOwnershipChange, ResourceOwnershipChangeKind, ResourceRef, WorkspaceKey,
};

pub const RESOURCE_INSTALLATION_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceInstallationState {
    Enabled,
    Disabled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResourceInstallationRecord {
    pub schema_version: u32,
    pub id: ResourceInstallationId,
    pub resource_id: String,
    pub source_id: String,
    pub resource_kind: ResourceKind,
    pub install_id: String,
    pub workspace_key: WorkspaceKey,
    pub agent_id: AgentId,
    pub effective_installation_id: InstallationId,
    pub canonical_project_path: String,
    pub adapter_contract: String,
    pub target_claim_id: PhysicalTargetId,
    pub state: ResourceInstallationState,
    pub ownership_record_ids: Vec<OwnershipRecordId>,
    pub created_by_receipt_id: ReceiptId,
    pub updated_by_receipt_id: ReceiptId,
}

#[derive(Debug, thiserror::Error)]
pub enum ResourceInstallationError {
    #[error("resource installation index is corrupt: {0}")]
    Corrupt(String),
    #[error("resource installation I/O failed: {0}")]
    Io(#[from] std::io::Error),
}

pub fn list_resource_installations(
) -> Result<Vec<ResourceInstallationRecord>, ResourceInstallationError> {
    let state = ExecutionState::open()?;
    load_installations(&state)
}

pub(super) fn list_resource_installations_for_lifecycle(
) -> Result<Vec<ResourceInstallationRecord>, ResourceInstallationError> {
    let state = ExecutionState::open()?;
    let mut records = load_installations(&state)?;
    project_legacy_skill_installations(&state, &mut records)?;
    records.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(records)
}

pub(super) fn installation_enabled(
    record: &ResourceInstallationRecord,
) -> Result<bool, ResourceInstallationError> {
    let state = ExecutionState::open()?;
    let name = installation_file_name(&record.id);
    match state.resource_installation_controls().read(&name) {
        Ok(bytes) => {
            let value: serde_json::Value = serde_json::from_slice(&bytes)
                .map_err(|error| ResourceInstallationError::Corrupt(error.to_string()))?;
            value
                .get("enabled")
                .and_then(serde_json::Value::as_bool)
                .ok_or_else(|| {
                    ResourceInstallationError::Corrupt(
                        "installation control has no enabled state".into(),
                    )
                })
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(true),
        Err(error) => Err(error.into()),
    }
}

pub(super) fn installation_control_path(
    installation_id: &ResourceInstallationId,
) -> Result<std::path::PathBuf, ResourceInstallationError> {
    let state = ExecutionState::open()?;
    let path = state
        .resource_installation_controls()
        .display_path()
        .join(installation_file_name(installation_id));
    let parent = std::fs::canonicalize(path.parent().unwrap_or_else(|| std::path::Path::new(".")))?;
    Ok(parent.join(path.file_name().unwrap_or_default()))
}

pub(super) fn installation_control_content(enabled: bool) -> serde_json::Value {
    serde_json::json!({"enabled": enabled})
}

pub(super) fn reconcile_installations(
    state: &ExecutionState,
    changes: &[ResourceOwnershipChange],
) -> Result<(), ResourceInstallationError> {
    for change in changes {
        let record = match change.kind {
            ResourceOwnershipChangeKind::Upsert => change.record.as_ref(),
            ResourceOwnershipChangeKind::Remove => change.previous_record.as_ref(),
        }
        .ok_or_else(|| {
            ResourceInstallationError::Corrupt("ownership change has no record".into())
        })?;
        let Some(catalog) = record.catalog_binding.as_ref() else {
            continue;
        };
        let installation_id = resource_installation_id(&record.resource);
        let file_name = installation_file_name(&installation_id);
        match change.kind {
            ResourceOwnershipChangeKind::Upsert => {
                let existing = read_installation(state, &installation_id)?;
                let installation = ResourceInstallationRecord {
                    schema_version: RESOURCE_INSTALLATION_SCHEMA_VERSION,
                    id: installation_id,
                    resource_id: catalog.resource_id.clone(),
                    source_id: catalog.source_id.clone(),
                    resource_kind: record.resource.kind,
                    install_id: catalog.install_id.clone(),
                    workspace_key: record.workspace_key.clone(),
                    agent_id: agent_id_for(&record.resource),
                    effective_installation_id: record.resource.installation_id.clone(),
                    canonical_project_path: record.resource.project_path.clone().ok_or_else(
                        || {
                            ResourceInstallationError::Corrupt(
                                "managed installation has no project path".into(),
                            )
                        },
                    )?,
                    adapter_contract: catalog.adapter_contract.clone(),
                    target_claim_id: record.target_id.clone(),
                    state: ResourceInstallationState::Enabled,
                    ownership_record_ids: vec![record.id.clone()],
                    created_by_receipt_id: existing
                        .as_ref()
                        .map(|item| item.created_by_receipt_id.clone())
                        .unwrap_or_else(|| record.creating_receipt_id.clone()),
                    updated_by_receipt_id: record.updated_by_receipt_id.clone(),
                };
                validate_installation(&installation)?;
                let bytes = serde_json::to_vec_pretty(&installation)
                    .map_err(|error| ResourceInstallationError::Corrupt(error.to_string()))?;
                if existing.is_some() {
                    state
                        .resource_installations()
                        .write_atomic(&file_name, &bytes)?;
                } else {
                    state
                        .resource_installations()
                        .write_atomic_new(&file_name, &bytes)?;
                }
            }
            ResourceOwnershipChangeKind::Remove => {
                match state.resource_installations().remove(&file_name) {
                    Ok(()) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error.into()),
                }
            }
        }
    }
    Ok(())
}

fn load_installations(
    state: &ExecutionState,
) -> Result<Vec<ResourceInstallationRecord>, ResourceInstallationError> {
    let mut records = Vec::new();
    for name in state.resource_installations().entry_names()? {
        let Some(name) = name.to_str().filter(|name| name.ends_with(".json")) else {
            continue;
        };
        let bytes = state.resource_installations().read(name)?;
        let record: ResourceInstallationRecord = serde_json::from_slice(&bytes)
            .map_err(|error| ResourceInstallationError::Corrupt(error.to_string()))?;
        validate_installation(&record)?;
        records.push(record);
    }
    records.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(records)
}

fn project_legacy_skill_installations(
    state: &ExecutionState,
    records: &mut Vec<ResourceInstallationRecord>,
) -> Result<(), ResourceInstallationError> {
    let catalog = load_resource_catalog_snapshot()
        .map_err(|error| ResourceInstallationError::Corrupt(error.to_string()))?;
    for name in state.ownership().entry_names()? {
        let Some(name) = name.to_str().filter(|name| name.ends_with(".json")) else {
            continue;
        };
        let Ok(candidate) = serde_json::from_slice::<super::ResourceOwnershipRecord>(
            &state.ownership().read(name)?,
        ) else {
            continue;
        };
        let Some(source_binding) = candidate.source_binding.as_ref() else {
            continue;
        };
        if candidate.resource.kind != ResourceKind::Skills {
            continue;
        }
        let Some(resource) = catalog.resources.values().find(|resource| {
            resource.source_id == source_binding.source_id
                && resource.kind == ResourceKind::Skills
                && resource.subpath == source_binding.skill_subpath
                && resource.present
        }) else {
            continue;
        };
        let id = resource_installation_id_for(
            &candidate.resource.installation_id,
            candidate
                .resource
                .project_path
                .as_deref()
                .unwrap_or_default(),
            ResourceKind::Skills,
            &resource.install_id,
        );
        if records.iter().any(|record| record.id == id) {
            continue;
        }
        let Some(ownership) = load_ownership_record_by_id(state, &candidate.id)
            .map_err(|error| ResourceInstallationError::Corrupt(error.to_string()))?
        else {
            continue;
        };
        let record = ResourceInstallationRecord {
            schema_version: RESOURCE_INSTALLATION_SCHEMA_VERSION,
            id,
            resource_id: resource.id.clone(),
            source_id: resource.source_id.clone(),
            resource_kind: ResourceKind::Skills,
            install_id: resource.install_id.clone(),
            workspace_key: ownership.workspace_key,
            agent_id: agent_id_for(&ownership.resource),
            effective_installation_id: ownership.resource.installation_id.clone(),
            canonical_project_path: ownership.resource.project_path.clone().ok_or_else(|| {
                ResourceInstallationError::Corrupt(
                    "legacy managed Skill installation has no project path".into(),
                )
            })?,
            adapter_contract: "project-skill-link-v1".into(),
            target_claim_id: ownership.target_id,
            state: ResourceInstallationState::Enabled,
            ownership_record_ids: vec![ownership.id],
            created_by_receipt_id: ownership.creating_receipt_id,
            updated_by_receipt_id: ownership.updated_by_receipt_id,
        };
        validate_installation(&record)?;
        records.push(record);
    }
    Ok(())
}

fn read_installation(
    state: &ExecutionState,
    id: &ResourceInstallationId,
) -> Result<Option<ResourceInstallationRecord>, ResourceInstallationError> {
    let bytes = match state
        .resource_installations()
        .read(&installation_file_name(id))
    {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let record = serde_json::from_slice(&bytes)
        .map_err(|error| ResourceInstallationError::Corrupt(error.to_string()))?;
    validate_installation(&record)?;
    Ok(Some(record))
}

fn validate_installation(
    record: &ResourceInstallationRecord,
) -> Result<(), ResourceInstallationError> {
    let valid = record.schema_version == RESOURCE_INSTALLATION_SCHEMA_VERSION
        && record.id
            == resource_installation_id_for(
                &record.effective_installation_id,
                &record.canonical_project_path,
                record.resource_kind,
                &record.install_id,
            )
        && record.target_claim_id.as_str().starts_with("target:")
        && !record.resource_id.is_empty()
        && !record.source_id.is_empty()
        && !record.install_id.is_empty()
        && !record.canonical_project_path.is_empty()
        && !record.ownership_record_ids.is_empty()
        && record
            .ownership_record_ids
            .iter()
            .collect::<BTreeSet<_>>()
            .len()
            == record.ownership_record_ids.len();
    if valid {
        Ok(())
    } else {
        Err(ResourceInstallationError::Corrupt(
            "installation identity is invalid".into(),
        ))
    }
}

fn resource_installation_id(resource: &ResourceRef) -> ResourceInstallationId {
    resource_installation_id_for(
        &resource.installation_id,
        resource.project_path.as_deref().unwrap_or_default(),
        resource.kind,
        resource.logical_id.rsplit('/').next().unwrap_or_default(),
    )
}

fn resource_installation_id_for(
    installation_id: &InstallationId,
    project_path: &str,
    kind: ResourceKind,
    install_id: &str,
) -> ResourceInstallationId {
    ResourceInstallationId::from(opaque_contract_id(
        "installation",
        &[
            installation_id.as_str(),
            project_path,
            kind.contract_name(),
            install_id,
        ],
    ))
}

fn installation_file_name(id: &ResourceInstallationId) -> String {
    format!("{}.json", id.as_str().replace(':', "_"))
}

fn agent_id_for(resource: &ResourceRef) -> AgentId {
    AgentId::from(
        resource
            .installation_id
            .as_str()
            .split(':')
            .next()
            .unwrap_or_default()
            .to_owned(),
    )
}
