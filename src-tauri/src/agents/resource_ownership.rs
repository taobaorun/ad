use std::path::Path;

use serde::{Deserialize, Serialize};

use super::execution_fs::directory_tree_digest;
use super::execution_state::ExecutionState;
use super::{
    opaque_contract_id, AgentContext, AgentError, AgentErrorCode, ContentDigest, OwnershipRecordId,
    PhysicalTargetId, ReceiptId, ResourceKind, ResourceRef, ResourceScope, ResourceStateKind,
    ResourceStorage, WorkspaceKey,
};

pub const RESOURCE_OWNERSHIP_SCHEMA_VERSION: u32 = 1;
pub const OWNERSHIP_EVIDENCE_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResourceOwnershipRecord {
    pub schema_version: u32,
    pub id: OwnershipRecordId,
    pub workspace_key: WorkspaceKey,
    pub resource: ResourceRef,
    pub target_id: PhysicalTargetId,
    pub target_path: String,
    pub target_kind: ResourceStateKind,
    pub target_digest: ContentDigest,
    pub artifact_id: String,
    pub artifact_digest: ContentDigest,
    pub creating_receipt_id: ReceiptId,
    pub updated_by_receipt_id: ReceiptId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceOwnershipChangeKind {
    Upsert,
    Remove,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResourceOwnershipChange {
    pub kind: ResourceOwnershipChangeKind,
    pub record_id: OwnershipRecordId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_record: Option<ResourceOwnershipRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub record: Option<ResourceOwnershipRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct OwnershipArtifact {
    pub id: String,
    pub digest: ContentDigest,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct OwnershipRestore {
    pub expected_current: Option<ResourceOwnershipRecord>,
    pub expected_matches_target: bool,
    pub restore: Option<ResourceOwnershipRecord>,
}

pub(super) fn ownership_managed(resource: &ResourceRef, storage: ResourceStorage) -> bool {
    resource.scope == ResourceScope::Project
        && resource.project_path.is_some()
        && matches!(
            (resource.kind, storage),
            (ResourceKind::Skills, ResourceStorage::Symlink)
                | (ResourceKind::Plugins, ResourceStorage::Directory)
        )
}

pub(super) fn ownership_record_id(resource: &ResourceRef) -> OwnershipRecordId {
    let target_id = PhysicalTargetId::for_resource(resource);
    OwnershipRecordId::from(opaque_contract_id("ownership", &[target_id.as_str()]))
}

pub(super) fn ownership_workspace_key(resource: &ResourceRef) -> Result<WorkspaceKey, AgentError> {
    workspace_key_for_context(&AgentContext {
        installation_id: resource.installation_id.clone(),
        project_path: resource.project_path.clone(),
    })
    .ok_or_else(|| {
        ownership_error(
            AgentErrorCode::InvalidPlan,
            resource,
            "Ownership-managed resources require a canonical project path",
        )
    })
}

pub(super) fn workspace_key_for_context(context: &AgentContext) -> Option<WorkspaceKey> {
    let project_path = context.project_path.as_deref()?;
    Some(WorkspaceKey::from(opaque_contract_id(
        "ownership-workspace",
        &[context.installation_id.as_str(), project_path],
    )))
}

pub(super) fn load_ownership_record(
    state: &ExecutionState,
    resource: &ResourceRef,
) -> Result<Option<ResourceOwnershipRecord>, AgentError> {
    let id = ownership_record_id(resource);
    let name = record_name(&id);
    let bytes = match state.ownership().read(&name) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(ownership_error(
                AgentErrorCode::Io,
                resource,
                format!("Failed to read ownership record: {error}"),
            ))
        }
    };
    let record: ResourceOwnershipRecord = serde_json::from_slice(&bytes).map_err(|error| {
        ownership_error(
            AgentErrorCode::PermissionDenied,
            resource,
            format!("Invalid ownership record: {error}"),
        )
    })?;
    validate_record_identity(&record, &id)?;
    if record.resource != *resource {
        return Err(ownership_error(
            AgentErrorCode::PermissionDenied,
            resource,
            "Ownership record identity does not match the resource",
        ));
    }
    Ok(Some(record))
}

pub(super) fn validate_ownership_record(
    record: &ResourceOwnershipRecord,
    resource: &ResourceRef,
    target_path: &Path,
    target_kind: ResourceStateKind,
    target_digest: Option<&ContentDigest>,
) -> Result<(), AgentError> {
    validate_ownership_record_identity(record, resource, target_path, target_kind)?;
    if Some(&record.target_digest) != target_digest {
        return Err(ownership_error(
            AgentErrorCode::PermissionDenied,
            resource,
            "Ownership record does not match the current physical target",
        ));
    }
    Ok(())
}

pub(super) fn validate_ownership_record_identity(
    record: &ResourceOwnershipRecord,
    resource: &ResourceRef,
    target_path: &Path,
    target_kind: ResourceStateKind,
) -> Result<(), AgentError> {
    let expected_workspace = ownership_workspace_key(resource)?;
    if record.workspace_key != expected_workspace
        || record.target_path != target_path.to_string_lossy()
        || record.target_kind != target_kind
    {
        return Err(ownership_error(
            AgentErrorCode::PermissionDenied,
            resource,
            "Ownership record does not match the current physical target",
        ));
    }
    Ok(())
}

pub(super) fn validate_ownership_artifact(
    record: &ResourceOwnershipRecord,
) -> Result<(), AgentError> {
    let path = Path::new(&record.artifact_id);
    let metadata = std::fs::symlink_metadata(path).map_err(|error| {
        ownership_error(
            AgentErrorCode::ResourceChanged,
            &record.resource,
            format!("Owned artifact is unavailable: {error}"),
        )
    })?;
    let digest = if metadata.file_type().is_symlink() {
        return Err(ownership_error(
            AgentErrorCode::ResourceChanged,
            &record.resource,
            "Owned artifact cannot be a symlink",
        ));
    } else if metadata.is_dir() {
        directory_tree_digest(path)
    } else if metadata.is_file() {
        std::fs::read(path).map(|bytes| ContentDigest::sha256(&bytes))
    } else {
        return Err(ownership_error(
            AgentErrorCode::ResourceChanged,
            &record.resource,
            "Owned artifact has an unsupported storage type",
        ));
    }
    .map_err(|error| {
        ownership_error(
            AgentErrorCode::ResourceChanged,
            &record.resource,
            format!("Failed to verify owned artifact: {error}"),
        )
    })?;
    if digest != record.artifact_digest {
        return Err(ownership_error(
            AgentErrorCode::ResourceChanged,
            &record.resource,
            "Owned artifact content changed",
        ));
    }
    Ok(())
}

pub(super) fn apply_ownership_changes(
    state: &ExecutionState,
    changes: &[ResourceOwnershipChange],
) -> Result<(), AgentError> {
    for change in changes {
        validate_ownership_change(change)?;
        let name = record_name(&change.record_id);
        let existing = match state.ownership().read(&name) {
            Ok(bytes) => Some(
                serde_json::from_slice::<ResourceOwnershipRecord>(&bytes).map_err(|error| {
                    AgentError {
                        code: AgentErrorCode::PermissionDenied,
                        message: format!("Invalid existing ownership record: {error}"),
                        agent_id: None,
                        installation_id: None,
                        resource: None,
                        retryable: false,
                        details: Some(serde_json::json!({"phase": "resource_ownership"})),
                    }
                })?,
            ),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => {
                return Err(AgentError {
                    code: AgentErrorCode::Io,
                    message: format!("Failed to read existing ownership record: {error}"),
                    agent_id: None,
                    installation_id: None,
                    resource: None,
                    retryable: false,
                    details: Some(serde_json::json!({"phase": "resource_ownership"})),
                })
            }
        };
        match change.kind {
            ResourceOwnershipChangeKind::Upsert => {
                let record = change.record.as_ref().ok_or_else(|| AgentError {
                    code: AgentErrorCode::InvalidPlan,
                    message: "Ownership upsert is missing its record".into(),
                    agent_id: None,
                    installation_id: None,
                    resource: None,
                    retryable: false,
                    details: Some(serde_json::json!({"phase": "resource_ownership"})),
                })?;
                if record.id != change.record_id {
                    return Err(ownership_error(
                        AgentErrorCode::InvalidPlan,
                        &record.resource,
                        "Ownership change identity does not match its record",
                    ));
                }
                if existing.as_ref() == Some(record) {
                    continue;
                }
                if existing != change.previous_record {
                    return Err(ownership_error(
                        AgentErrorCode::PermissionDenied,
                        &record.resource,
                        "Existing ownership record changed before reconciliation",
                    ));
                }
                let bytes = serde_json::to_vec_pretty(record).map_err(|error| {
                    ownership_error(AgentErrorCode::Io, &record.resource, error.to_string())
                })?;
                if change.previous_record.is_some() {
                    state
                        .ownership()
                        .write_atomic(&name, &bytes)
                        .map_err(|error| {
                            ownership_error(AgentErrorCode::Io, &record.resource, error.to_string())
                        })?;
                } else {
                    state
                        .ownership()
                        .write_atomic_new(&name, &bytes)
                        .map_err(|error| {
                            ownership_error(AgentErrorCode::Io, &record.resource, error.to_string())
                        })?;
                }
            }
            ResourceOwnershipChangeKind::Remove => {
                if existing.is_none() {
                    continue;
                }
                if existing != change.previous_record || change.previous_record.is_none() {
                    return Err(AgentError {
                        code: AgentErrorCode::PermissionDenied,
                        message: "Existing ownership record changed before removal".into(),
                        agent_id: None,
                        installation_id: None,
                        resource: None,
                        retryable: false,
                        details: Some(serde_json::json!({"phase": "resource_ownership"})),
                    });
                }
                state
                    .ownership()
                    .remove(&name)
                    .map_err(|error| AgentError {
                        code: AgentErrorCode::Io,
                        message: format!("Failed to remove ownership record: {error}"),
                        agent_id: None,
                        installation_id: None,
                        resource: None,
                        retryable: false,
                        details: Some(serde_json::json!({"phase": "resource_ownership"})),
                    })?;
            }
        }
    }
    Ok(())
}

fn record_name(id: &OwnershipRecordId) -> String {
    format!("{id}.json")
}

fn validate_ownership_change(change: &ResourceOwnershipChange) -> Result<(), AgentError> {
    if let Some(previous) = change.previous_record.as_ref() {
        validate_record_identity(previous, &change.record_id)?;
    }
    match change.kind {
        ResourceOwnershipChangeKind::Upsert => {
            let record = change.record.as_ref().ok_or_else(|| AgentError {
                code: AgentErrorCode::InvalidPlan,
                message: "Ownership upsert is missing its record".into(),
                agent_id: None,
                installation_id: None,
                resource: None,
                retryable: false,
                details: Some(serde_json::json!({"phase": "resource_ownership"})),
            })?;
            validate_record_identity(record, &change.record_id)?;
            if change.previous_record.as_ref().is_some_and(|previous| {
                previous.resource != record.resource
                    || previous.target_id != record.target_id
                    || previous.target_path != record.target_path
            }) {
                return Err(ownership_error(
                    AgentErrorCode::InvalidPlan,
                    &record.resource,
                    "Ownership upsert changes the physical resource identity",
                ));
            }
        }
        ResourceOwnershipChangeKind::Remove => {
            if change.record.is_some() || change.previous_record.is_none() {
                return Err(AgentError {
                    code: AgentErrorCode::InvalidPlan,
                    message: "Ownership removal requires only the previous record".into(),
                    agent_id: None,
                    installation_id: None,
                    resource: None,
                    retryable: false,
                    details: Some(serde_json::json!({"phase": "resource_ownership"})),
                });
            }
        }
    }
    Ok(())
}

fn validate_record_identity(
    record: &ResourceOwnershipRecord,
    expected_id: &OwnershipRecordId,
) -> Result<(), AgentError> {
    let valid_kind = matches!(
        record.target_kind,
        ResourceStateKind::Symlink | ResourceStateKind::Directory
    );
    let valid_paths = Path::new(&record.target_path).is_absolute()
        && Path::new(&record.artifact_id).is_absolute();
    let valid_identity = record.schema_version == RESOURCE_OWNERSHIP_SCHEMA_VERSION
        && &record.id == expected_id
        && record.target_id == PhysicalTargetId::for_resource(&record.resource)
        && record.workspace_key == ownership_workspace_key(&record.resource)?
        && record.resource.scope == ResourceScope::Project
        && record.resource.project_path.is_some();
    if valid_identity && valid_kind && valid_paths {
        Ok(())
    } else {
        Err(ownership_error(
            AgentErrorCode::PermissionDenied,
            &record.resource,
            "Ownership record identity is invalid",
        ))
    }
}

fn ownership_error(
    code: AgentErrorCode,
    resource: &ResourceRef,
    message: impl Into<String>,
) -> AgentError {
    AgentError {
        code,
        message: message.into(),
        agent_id: None,
        installation_id: Some(resource.installation_id.clone()),
        resource: Some(resource.clone()),
        retryable: code == AgentErrorCode::ResourceChanged,
        details: Some(serde_json::json!({"phase": "resource_ownership"})),
    }
}
