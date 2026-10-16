use std::os::unix::fs::symlink;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::execution_fs::directory_tree_digest;
use super::execution_state::ExecutionState;
use super::{
    load_skill_catalog_snapshot, opaque_contract_id, resolve_skill_source_item, AgentContext,
    AgentError, AgentErrorCode, ContentDigest, OwnershipRecordId, PhysicalTargetId, ReceiptId,
    ResourceKind, ResourceRef, ResourceScope, ResourceStateKind, ResourceStorage, SkillSourceType,
    WorkspaceKey,
};

pub const RESOURCE_OWNERSHIP_SCHEMA_VERSION: u32 = 3;
const LEGACY_RESOURCE_OWNERSHIP_SCHEMA_VERSION: u32 = 1;
const SKILL_RESOURCE_OWNERSHIP_SCHEMA_VERSION: u32 = 2;
pub const OWNERSHIP_EVIDENCE_VERSION: u32 = 1;
const SOURCE_TRACKING_MIGRATION_JOURNAL_VERSION: u32 = 1;

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_binding: Option<SkillOwnershipBinding>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub catalog_binding: Option<CatalogOwnershipBinding>,
    pub creating_receipt_id: ReceiptId,
    pub updated_by_receipt_id: ReceiptId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SkillOwnershipBinding {
    pub source_id: String,
    pub source_type: SkillSourceType,
    pub binding_id: String,
    pub stable_root: String,
    pub skill_subpath: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CatalogOwnershipBinding {
    pub resource_id: String,
    pub source_id: String,
    pub install_id: String,
    pub resource_kind: ResourceKind,
    pub stable_root: String,
    pub resource_subpath: String,
    pub adapter_contract: String,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct OwnershipMigrationJournal {
    schema_version: u32,
    record_id: OwnershipRecordId,
    target_path: String,
    source_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    backup_path: Option<String>,
    previous_kind: ResourceStateKind,
    previous_digest: ContentDigest,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct OwnershipArtifact {
    pub id: String,
    pub digest: ContentDigest,
    pub source_binding: Option<SkillOwnershipBinding>,
    pub catalog_binding: Option<CatalogOwnershipBinding>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct OwnershipRestore {
    pub expected_current: Option<ResourceOwnershipRecord>,
    pub expected_matches_target: bool,
    pub restore: Option<ResourceOwnershipRecord>,
}

pub(super) fn ownership_managed(resource: &ResourceRef, storage: ResourceStorage) -> bool {
    let scope_matches = match resource.scope {
        ResourceScope::User => resource.project_path.is_none(),
        ResourceScope::Project => resource.project_path.is_some(),
    };
    scope_matches
        && (matches!(
            (resource.kind, storage),
            (ResourceKind::Skills, ResourceStorage::Symlink)
                | (ResourceKind::Plugins, ResourceStorage::Directory)
        ) || (resource.kind == ResourceKind::Plugins
            && storage == ResourceStorage::Symlink
            && resource.logical_id.starts_with("skill-source:")))
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
            "Invalid ownership context",
        )
    })
}

pub(super) fn workspace_key_for_context(context: &AgentContext) -> Option<WorkspaceKey> {
    match context.project_path.as_deref() {
        Some(project_path) => super::resolve_project_agent_workspace(
            &context.installation_id,
            Path::new(project_path),
        )
        .ok()
        .map(|workspace| workspace.key),
        None => super::resolve_user_agent_workspace(&context.installation_id)
            .ok()
            .map(|workspace| workspace.key),
    }
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

pub(super) fn load_ownership_record_by_id(
    state: &ExecutionState,
    id: &OwnershipRecordId,
) -> Result<Option<ResourceOwnershipRecord>, AgentError> {
    let name = record_name(id);
    let bytes = match state.ownership().read(&name) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(AgentError {
                code: AgentErrorCode::Io,
                message: format!("Failed to read ownership record: {error}"),
                agent_id: None,
                installation_id: None,
                resource: None,
                retryable: false,
                details: Some(serde_json::json!({"phase": "resource_ownership"})),
            })
        }
    };
    let record: ResourceOwnershipRecord =
        serde_json::from_slice(&bytes).map_err(|error| AgentError {
            code: AgentErrorCode::PermissionDenied,
            message: format!("Invalid ownership record: {error}"),
            agent_id: None,
            installation_id: None,
            resource: None,
            retryable: false,
            details: Some(serde_json::json!({"phase": "resource_ownership"})),
        })?;
    validate_record_identity(&record, id)?;
    Ok(Some(record))
}

pub fn migrate_legacy_source_tracking_ownership() -> Result<usize, AgentError> {
    let state = ExecutionState::open().map_err(|error| AgentError {
        code: AgentErrorCode::Io,
        message: error.to_string(),
        agent_id: None,
        installation_id: None,
        resource: None,
        retryable: false,
        details: Some(serde_json::json!({"phase": "source_tracking_migration"})),
    })?;
    let catalog = super::load_resource_catalog_snapshot().map_err(|error| AgentError {
        code: AgentErrorCode::Io,
        message: error.to_string(),
        agent_id: None,
        installation_id: None,
        resource: None,
        retryable: false,
        details: Some(serde_json::json!({"phase": "source_tracking_migration"})),
    })?;
    let mut reconciled_ownership = super::list_resource_installations()
        .unwrap_or_default()
        .into_iter()
        .flat_map(|record| {
            let contract = record.adapter_contract;
            record
                .ownership_record_ids
                .into_iter()
                .map(move |id| (id, contract.clone()))
        })
        .collect::<std::collections::BTreeSet<_>>();
    let mut migrated = 0;
    for name in state
        .ownership()
        .entry_names()
        .map_err(|error| AgentError {
            code: AgentErrorCode::Io,
            message: error.to_string(),
            agent_id: None,
            installation_id: None,
            resource: None,
            retryable: false,
            details: Some(serde_json::json!({"phase": "source_tracking_migration"})),
        })?
    {
        let Some(name) = name.to_str().filter(|name| name.ends_with(".json")) else {
            continue;
        };
        let bytes = state.ownership().read(name).map_err(|error| AgentError {
            code: AgentErrorCode::Io,
            message: error.to_string(),
            agent_id: None,
            installation_id: None,
            resource: None,
            retryable: false,
            details: Some(serde_json::json!({"phase": "source_tracking_migration"})),
        })?;
        let Ok(mut record) = serde_json::from_slice::<ResourceOwnershipRecord>(&bytes) else {
            continue;
        };
        if record.catalog_binding.as_ref().is_some_and(|binding| {
            matches!(
                binding.adapter_contract.as_str(),
                "user-skill-link-v1"
                    | "project-skill-link-v1"
                    | "codex-plugin-link-v1"
                    | "claude-plugin-link-v1"
            )
        }) {
            if current_link_matches_record(&record) {
                let binding = record.catalog_binding.as_ref().unwrap();
                let operation_id = format!("source-tracking-recovery:{}", uuid::Uuid::new_v4());
                let _lock = super::TargetLockSet::acquire_for_ad_states(
                    &[
                        PathBuf::from(&record.target_path),
                        state.ownership().display_path().join(name),
                        super::resource_lifecycle_lock_target(&state, &binding.resource_id),
                    ],
                    &operation_id,
                    &state,
                )
                .map_err(|error| {
                    ownership_error(AgentErrorCode::Io, &record.resource, error.to_string())
                })?;
                let still_current = state
                    .ownership()
                    .read(name)
                    .ok()
                    .and_then(|bytes| {
                        serde_json::from_slice::<ResourceOwnershipRecord>(&bytes).ok()
                    })
                    .is_some_and(|current| {
                        current == record && current_link_matches_record(&current)
                    });
                if !still_current {
                    continue;
                }
                let adapter_contract = record
                    .catalog_binding
                    .as_ref()
                    .map(|binding| binding.adapter_contract.clone())
                    .unwrap_or_default();
                let reconciliation_key = (record.id.clone(), adapter_contract);
                if !reconciled_ownership.contains(&reconciliation_key) {
                    super::reconcile_installations(
                        &state,
                        &[ResourceOwnershipChange {
                            kind: ResourceOwnershipChangeKind::Upsert,
                            record_id: record.id.clone(),
                            previous_record: Some(record.clone()),
                            record: Some(record.clone()),
                        }],
                    )
                    .map_err(migration_reconciliation_error)?;
                    reconciled_ownership.insert(reconciliation_key);
                }
                if let Some(journal) = load_ownership_migration_journal(&state, &record.id)? {
                    validate_ownership_migration_journal(&record, &journal)?;
                    finish_journaled_target(&record, &journal)?;
                    remove_ownership_migration_journal(&state, &record.id)?;
                }
            }
            continue;
        }
        let candidate = record
            .catalog_binding
            .as_ref()
            .and_then(|binding| catalog.resources.get(&binding.resource_id))
            .or_else(|| {
                record.source_binding.as_ref().and_then(|binding| {
                    catalog.resources.values().find(|resource| {
                        resource.kind == record.resource.kind
                            && resource.source_id == binding.source_id
                            && resource.subpath == binding.skill_subpath
                    })
                })
            })
            .or_else(|| {
                let (source_id, install_id) = record.resource.logical_id.rsplit_once('/')?;
                catalog.resources.values().find(|resource| {
                    resource.kind == record.resource.kind
                        && resource.source_id == source_id
                        && resource.install_id == install_id
                })
            });
        let Some(candidate) = candidate.filter(|candidate| {
            candidate.present && candidate.lifecycle == super::ResourceLifecycle::Managed
        }) else {
            continue;
        };
        let legacy_contract = record
            .catalog_binding
            .as_ref()
            .map(|binding| binding.adapter_contract.as_str());
        let migratable = (record.resource.kind == ResourceKind::Skills
            && record.artifact_id
                != catalog
                    .sources
                    .get(&candidate.source_id)
                    .and_then(|source| source.binding.as_ref())
                    .map(|binding| {
                        Path::new(&binding.stable_root)
                            .join(&candidate.subpath)
                            .to_string_lossy()
                            .into_owned()
                    })
                    .unwrap_or_default())
            || legacy_contract == Some("codex-plugin-store-v1");
        let migratable = migratable
            || (legacy_contract == Some("claude-plugin-dir-v1")
                && record.target_kind == ResourceStateKind::Symlink);
        if !migratable {
            continue;
        }
        let target = PathBuf::from(&record.target_path);
        let operation_id = format!("source-tracking-migration:{}", uuid::Uuid::new_v4());
        let _lock = super::TargetLockSet::acquire_for_ad_states(
            &[
                target.clone(),
                state.ownership().display_path().join(name),
                super::resource_lifecycle_lock_target(&state, &candidate.id),
            ],
            &operation_id,
            &state,
        )
        .map_err(|error| {
            ownership_error(AgentErrorCode::Io, &record.resource, error.to_string())
        })?;
        let current_record = state
            .ownership()
            .read(name)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<ResourceOwnershipRecord>(&bytes).ok());
        if current_record.as_ref() != Some(&record) {
            continue;
        }
        let current_catalog = super::load_resource_catalog_snapshot().map_err(|error| {
            ownership_error(
                AgentErrorCode::ResourceChanged,
                &record.resource,
                error.to_string(),
            )
        })?;
        let Some(candidate) = current_catalog
            .resources
            .get(&candidate.id)
            .filter(|candidate| {
                candidate.present && candidate.lifecycle == super::ResourceLifecycle::Managed
            })
        else {
            continue;
        };
        let resolved = super::resolve_catalog_resource(&candidate.id).map_err(|error| {
            ownership_error(
                AgentErrorCode::ResourceChanged,
                &record.resource,
                error.to_string(),
            )
        })?;
        let source = resolved.stable_path;
        let source_digest =
            super::directory_tree_digest(&resolved.physical_path).map_err(|error| {
                ownership_error(
                    AgentErrorCode::ResourceChanged,
                    &record.resource,
                    error.to_string(),
                )
            })?;
        let existing_journal = load_ownership_migration_journal(&state, &record.id)?;
        if let Some(journal) = existing_journal.as_ref() {
            validate_ownership_migration_journal(&record, journal)?;
            recover_journaled_target(&record, journal);
        }
        let already_linked = std::fs::read_link(&target)
            .ok()
            .is_some_and(|link| link == source);
        if !already_linked && !legacy_target_matches_record(&record) {
            continue;
        }
        let journal = match existing_journal {
            Some(journal) => journal,
            None => begin_ownership_migration_journal(&state, &record, &target, &source)?,
        };
        let rollback = if already_linked {
            journal
                .backup_path
                .as_ref()
                .map(PathBuf::from)
                .map(TargetMigrationRollback::Directory)
                .unwrap_or_else(|| TargetMigrationRollback::Link(source.clone()))
        } else {
            migrate_target_to_link(&record, &target, &source, journal.backup_path.as_deref())?
        };
        let previous = record.clone();
        record.schema_version = RESOURCE_OWNERSHIP_SCHEMA_VERSION;
        record.target_kind = ResourceStateKind::Symlink;
        record.target_digest = ContentDigest::sha256(source.to_string_lossy().as_bytes());
        record.artifact_id = source.to_string_lossy().into_owned();
        record.artifact_digest = source_digest;
        let stable_root = current_catalog
            .sources
            .get(&candidate.source_id)
            .and_then(|source| source.binding.as_ref())
            .map(|binding| binding.stable_root.clone())
            .unwrap_or_else(|| source.to_string_lossy().into_owned());
        record.catalog_binding = Some(CatalogOwnershipBinding {
            resource_id: candidate.id.clone(),
            source_id: candidate.source_id.clone(),
            install_id: candidate.install_id.clone(),
            resource_kind: candidate.kind,
            stable_root,
            resource_subpath: candidate.subpath.clone(),
            adapter_contract: match (record.resource.kind, record.resource.scope) {
                (ResourceKind::Skills, ResourceScope::User) => "user-skill-link-v1".into(),
                (ResourceKind::Skills, ResourceScope::Project) => "project-skill-link-v1".into(),
                (ResourceKind::Plugins, _)
                    if record
                        .resource
                        .installation_id
                        .as_str()
                        .starts_with("codex:") =>
                {
                    "codex-plugin-link-v1".into()
                }
                (ResourceKind::Plugins, _) => "claude-plugin-link-v1".into(),
                _ => unreachable!(),
            },
        });
        let rendered = serde_json::to_vec_pretty(&record).map_err(|error| {
            ownership_error(AgentErrorCode::Io, &record.resource, error.to_string())
        })?;
        if let Err(error) = state.ownership().write_atomic(name, &rendered) {
            rollback_migrated_target(&target, rollback);
            return Err(ownership_error(
                AgentErrorCode::Io,
                &record.resource,
                error.to_string(),
            ));
        }
        super::reconcile_installations(
            &state,
            &[ResourceOwnershipChange {
                kind: ResourceOwnershipChangeKind::Upsert,
                record_id: record.id.clone(),
                previous_record: Some(previous),
                record: Some(record.clone()),
            }],
        )
        .map_err(migration_reconciliation_error)?;
        drop(rollback);
        finish_journaled_target(&record, &journal)?;
        remove_ownership_migration_journal(&state, &record.id)?;
        migrated += 1;
    }
    Ok(migrated)
}

fn migration_reconciliation_error(error: impl ToString) -> AgentError {
    AgentError {
        code: AgentErrorCode::Io,
        message: error.to_string(),
        agent_id: None,
        installation_id: None,
        resource: None,
        retryable: false,
        details: Some(serde_json::json!({"phase": "source_tracking_migration"})),
    }
}

fn current_link_matches_record(record: &ResourceOwnershipRecord) -> bool {
    std::fs::read_link(&record.target_path)
        .ok()
        .is_some_and(|link| link == Path::new(&record.artifact_id))
}

fn ownership_migration_journal_name(id: &OwnershipRecordId) -> String {
    format!("ownership-{}.json", id.as_str().replace(':', "_"))
}

fn load_ownership_migration_journal(
    state: &ExecutionState,
    id: &OwnershipRecordId,
) -> Result<Option<OwnershipMigrationJournal>, AgentError> {
    let bytes = match state
        .source_tracking_migration_journals()
        .read(&ownership_migration_journal_name(id))
    {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(migration_reconciliation_error(error)),
    };
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(migration_reconciliation_error)
}

fn begin_ownership_migration_journal(
    state: &ExecutionState,
    record: &ResourceOwnershipRecord,
    target: &Path,
    source: &Path,
) -> Result<OwnershipMigrationJournal, AgentError> {
    let backup_path = (record.target_kind == ResourceStateKind::Directory).then(|| {
        target
            .with_file_name(format!(
                ".{}.ad-source-migration-{}",
                target
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("resource"),
                uuid::Uuid::new_v4()
            ))
            .to_string_lossy()
            .into_owned()
    });
    let journal = OwnershipMigrationJournal {
        schema_version: SOURCE_TRACKING_MIGRATION_JOURNAL_VERSION,
        record_id: record.id.clone(),
        target_path: target.to_string_lossy().into_owned(),
        source_path: source.to_string_lossy().into_owned(),
        backup_path,
        previous_kind: record.target_kind,
        previous_digest: record.target_digest.clone(),
    };
    let bytes = serde_json::to_vec_pretty(&journal).map_err(migration_reconciliation_error)?;
    state
        .source_tracking_migration_journals()
        .write_atomic_new(&ownership_migration_journal_name(&record.id), &bytes)
        .map_err(migration_reconciliation_error)?;
    Ok(journal)
}

fn validate_ownership_migration_journal(
    record: &ResourceOwnershipRecord,
    journal: &OwnershipMigrationJournal,
) -> Result<(), AgentError> {
    let target = Path::new(&journal.target_path);
    let backup_valid = journal.backup_path.as_deref().map_or(true, |backup| {
        let backup = Path::new(backup);
        backup.parent() == target.parent()
            && backup.file_name().is_some_and(|name| {
                name.to_string_lossy().starts_with(&format!(
                    ".{}.ad-source-migration-",
                    target
                        .file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or("resource")
                ))
            })
    });
    let record_valid = (record.target_kind == journal.previous_kind
        && record.target_digest == journal.previous_digest)
        || (record.target_kind == ResourceStateKind::Symlink
            && record.artifact_id == journal.source_path);
    if journal.schema_version == SOURCE_TRACKING_MIGRATION_JOURNAL_VERSION
        && journal.record_id == record.id
        && journal.target_path == record.target_path
        && Path::new(&journal.source_path).is_absolute()
        && backup_valid
        && record_valid
    {
        Ok(())
    } else {
        Err(ownership_error(
            AgentErrorCode::PermissionDenied,
            &record.resource,
            "Source-tracking migration journal is invalid",
        ))
    }
}

fn recover_journaled_target(record: &ResourceOwnershipRecord, journal: &OwnershipMigrationJournal) {
    let target = Path::new(&journal.target_path);
    if std::fs::symlink_metadata(target).is_ok() {
        return;
    }
    if let Some(backup) = journal.backup_path.as_deref() {
        let backup = Path::new(backup);
        if migration_directory_digest(record, backup).ok() == Some(journal.previous_digest.clone())
        {
            let _ = std::fs::rename(backup, target);
        }
    }
}

fn finish_journaled_target(
    record: &ResourceOwnershipRecord,
    journal: &OwnershipMigrationJournal,
) -> Result<(), AgentError> {
    let Some(backup) = journal.backup_path.as_deref() else {
        return Ok(());
    };
    let backup = Path::new(backup);
    if std::fs::symlink_metadata(backup).is_err() {
        return Ok(());
    }
    if migration_directory_digest(record, backup).ok() != Some(journal.previous_digest.clone()) {
        return Err(migration_reconciliation_error(
            "Source-tracking migration backup changed",
        ));
    }
    std::fs::remove_dir_all(backup).map_err(migration_reconciliation_error)
}

fn migration_directory_digest(
    record: &ResourceOwnershipRecord,
    path: &Path,
) -> Result<ContentDigest, std::io::Error> {
    if record.resource.kind == ResourceKind::Plugins
        && record
            .resource
            .installation_id
            .as_str()
            .starts_with("codex:")
    {
        super::execution_fs::directory_tree_digest_filtered(path, |entry| {
            Ok(!super::resource_scanner::is_agent_skill_projection(entry))
        })
    } else {
        super::directory_tree_digest(path)
    }
}

fn remove_ownership_migration_journal(
    state: &ExecutionState,
    id: &OwnershipRecordId,
) -> Result<(), AgentError> {
    state
        .source_tracking_migration_journals()
        .remove(&ownership_migration_journal_name(id))
        .map_err(migration_reconciliation_error)
}

enum TargetMigrationRollback {
    Link(PathBuf),
    Directory(PathBuf),
}

fn legacy_target_matches_record(record: &ResourceOwnershipRecord) -> bool {
    let target = Path::new(&record.target_path);
    let Ok(metadata) = std::fs::symlink_metadata(target) else {
        return false;
    };
    let (kind, digest) = if metadata.file_type().is_symlink() {
        let Ok(link) = std::fs::read_link(target) else {
            return false;
        };
        (
            ResourceStateKind::Symlink,
            ContentDigest::sha256(link.to_string_lossy().as_bytes()),
        )
    } else if metadata.is_dir() {
        let Ok(digest) = super::directory_tree_digest(target) else {
            return false;
        };
        (ResourceStateKind::Directory, digest)
    } else {
        return false;
    };
    validate_ownership_record(record, &record.resource, target, kind, Some(&digest)).is_ok()
}

fn migrate_target_to_link(
    record: &ResourceOwnershipRecord,
    target: &Path,
    source: &Path,
    backup_path: Option<&str>,
) -> Result<TargetMigrationRollback, AgentError> {
    let metadata = std::fs::symlink_metadata(target).map_err(|error| {
        ownership_error(
            AgentErrorCode::ResourceChanged,
            &record.resource,
            error.to_string(),
        )
    })?;
    if metadata.file_type().is_symlink() {
        let previous = std::fs::read_link(target).map_err(|error| {
            ownership_error(AgentErrorCode::Io, &record.resource, error.to_string())
        })?;
        replace_symlink(target, source).map_err(|error| {
            ownership_error(AgentErrorCode::Io, &record.resource, error.to_string())
        })?;
        Ok(TargetMigrationRollback::Link(previous))
    } else if metadata.is_dir() {
        let backup = backup_path.map(PathBuf::from).ok_or_else(|| {
            ownership_error(
                AgentErrorCode::InvalidPlan,
                &record.resource,
                "Directory migration journal has no backup path",
            )
        })?;
        std::fs::rename(target, &backup).map_err(|error| {
            ownership_error(AgentErrorCode::Io, &record.resource, error.to_string())
        })?;
        if let Err(error) = symlink(source, target) {
            let _ = std::fs::rename(&backup, target);
            return Err(ownership_error(
                AgentErrorCode::Io,
                &record.resource,
                error.to_string(),
            ));
        }
        Ok(TargetMigrationRollback::Directory(backup))
    } else {
        Err(ownership_error(
            AgentErrorCode::PermissionDenied,
            &record.resource,
            "Legacy managed target is neither a link nor directory",
        ))
    }
}

fn replace_symlink(target: &Path, source: &Path) -> std::io::Result<()> {
    let temp = target.with_file_name(format!(
        ".{}.ad-source-link-{}",
        target
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("resource"),
        uuid::Uuid::new_v4()
    ));
    symlink(source, &temp)?;
    match std::fs::rename(&temp, target) {
        Ok(()) => Ok(()),
        Err(error) => {
            let _ = std::fs::remove_file(temp);
            Err(error)
        }
    }
}

fn rollback_migrated_target(target: &Path, rollback: TargetMigrationRollback) {
    match rollback {
        TargetMigrationRollback::Link(previous) => {
            let _ = replace_symlink(target, &previous);
        }
        TargetMigrationRollback::Directory(backup) => {
            let _ = std::fs::remove_file(target);
            let _ = std::fs::rename(backup, target);
        }
    }
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
    if let Some(source_binding) = &record.source_binding {
        return validate_owned_skill_binding(record, source_binding);
    }
    if let Some(catalog_binding) = &record.catalog_binding {
        return validate_owned_catalog_binding(record, catalog_binding);
    }
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

pub(super) fn validate_ownership_target(
    record: &ResourceOwnershipRecord,
) -> Result<(), AgentError> {
    let target = Path::new(&record.target_path);
    let metadata = std::fs::symlink_metadata(target).map_err(|error| {
        ownership_error(
            AgentErrorCode::ResourceChanged,
            &record.resource,
            format!("Owned target is unavailable: {error}"),
        )
    })?;
    let kind = if metadata.file_type().is_symlink() {
        ResourceStateKind::Symlink
    } else if metadata.is_dir() {
        ResourceStateKind::Directory
    } else if metadata.is_file() {
        ResourceStateKind::File
    } else {
        return Err(ownership_error(
            AgentErrorCode::PermissionDenied,
            &record.resource,
            "Owned target has an unsupported storage type",
        ));
    };
    let digest = match kind {
        ResourceStateKind::Symlink => Some(ContentDigest::sha256(
            std::fs::read_link(target)
                .map_err(|error| {
                    ownership_error(AgentErrorCode::Io, &record.resource, error.to_string())
                })?
                .to_string_lossy()
                .as_bytes(),
        )),
        ResourceStateKind::Directory => {
            Some(super::directory_tree_digest(target).map_err(|error| {
                ownership_error(AgentErrorCode::Io, &record.resource, error.to_string())
            })?)
        }
        ResourceStateKind::File => Some(ContentDigest::sha256(&std::fs::read(target).map_err(
            |error| ownership_error(AgentErrorCode::Io, &record.resource, error.to_string()),
        )?)),
        ResourceStateKind::Missing => None,
    };
    validate_ownership_record(record, &record.resource, target, kind, digest.as_ref())
}

pub(super) fn ownership_source_binding(
    resource: &ResourceRef,
    artifact_id: &str,
) -> Result<Option<SkillOwnershipBinding>, AgentError> {
    if resource.kind != ResourceKind::Skills {
        return Ok(None);
    }
    if resource.scope == ResourceScope::User {
        return Ok(None);
    }
    let Some((source_id, logical_id)) = resource.logical_id.rsplit_once('/') else {
        return Ok(None);
    };
    let catalog = load_skill_catalog_snapshot().map_err(|error| {
        ownership_error(
            AgentErrorCode::InvalidPlan,
            resource,
            format!("Skill catalog is unavailable: {error}"),
        )
    })?;
    let Some(entry) = catalog
        .entries
        .iter()
        .find(|entry| entry.source_id == source_id)
    else {
        return Ok(None);
    };
    let Some(binding) = &entry.current_binding else {
        return Ok(None);
    };
    let item = binding
        .skills
        .iter()
        .find(|item| item.logical_id == logical_id)
        .ok_or_else(|| {
            ownership_error(
                AgentErrorCode::InvalidPlan,
                resource,
                "Catalog Skill binding no longer contains the requested Skill",
            )
        })?;
    let (stable_skill, _) = resolve_skill_source_item(binding, item).map_err(|error| {
        ownership_error(AgentErrorCode::InvalidPlan, resource, error.to_string())
    })?;
    if stable_skill != Path::new(artifact_id) {
        return Err(ownership_error(
            AgentErrorCode::InvalidPlan,
            resource,
            "Skill link target does not match its catalog source binding",
        ));
    }
    Ok(Some(SkillOwnershipBinding {
        source_id: binding.source_id.clone(),
        source_type: binding.source_type,
        binding_id: binding.binding_id.clone(),
        stable_root: binding.stable_root.clone(),
        skill_subpath: item.subpath.clone(),
    }))
}

pub(super) fn ownership_catalog_binding(
    resource: &ResourceRef,
    artifact_id: &str,
) -> Result<Option<CatalogOwnershipBinding>, AgentError> {
    if !matches!(resource.kind, ResourceKind::Skills | ResourceKind::Plugins) {
        return Ok(None);
    }
    let Some((source_id, install_id)) = resource.logical_id.rsplit_once('/') else {
        return Ok(None);
    };
    let catalog = super::load_resource_catalog_snapshot().map_err(|error| {
        ownership_error(
            AgentErrorCode::InvalidPlan,
            resource,
            format!("Resource catalog is unavailable: {error}"),
        )
    })?;
    let candidate = catalog
        .resources
        .values()
        .find(|candidate| {
            candidate.source_id == source_id
                && candidate.install_id == install_id
                && candidate.kind == resource.kind
                && candidate.present
                && candidate.lifecycle == super::ResourceLifecycle::Managed
        })
        .ok_or_else(|| {
            ownership_error(
                AgentErrorCode::InvalidPlan,
                resource,
                "Catalog resource is unavailable",
            )
        })?;
    let resolved = super::resolve_catalog_resource(&candidate.id).map_err(|error| {
        ownership_error(AgentErrorCode::InvalidPlan, resource, error.to_string())
    })?;
    let source = catalog.sources.get(source_id).ok_or_else(|| {
        ownership_error(
            AgentErrorCode::InvalidPlan,
            resource,
            "Catalog source is unavailable",
        )
    })?;
    let user_skill = resource.kind == ResourceKind::Skills && resource.scope == ResourceScope::User;
    let linked_user_skill = user_skill && resolved.stable_path == Path::new(artifact_id);
    let expected_artifact = if user_skill && !linked_user_skill {
        let artifact = Path::new(artifact_id);
        let artifacts_root = crate::fs::paths::skill_artifacts_dir()
            .map_err(|error| ownership_error(AgentErrorCode::Io, resource, error.to_string()))?;
        if !artifact.starts_with(artifacts_root)
            || !artifact.is_dir()
            || !artifact.join("SKILL.md").is_file()
        {
            return Err(ownership_error(
                AgentErrorCode::InvalidPlan,
                resource,
                "User Skill artifact is outside the managed immutable store",
            ));
        }
        artifact.to_path_buf()
    } else {
        resolved.stable_path.clone()
    };
    if expected_artifact != Path::new(artifact_id) {
        return Err(ownership_error(
            AgentErrorCode::InvalidPlan,
            resource,
            "Collection link target does not match its catalog resource",
        ));
    }
    let binding = source.binding.as_ref().ok_or_else(|| {
        ownership_error(
            AgentErrorCode::InvalidPlan,
            resource,
            "Catalog source binding is unavailable",
        )
    })?;
    if binding.binding_id.starts_with("skill-artifact:") {
        return Ok(None);
    }
    let adapter_contract = match (resource.kind, resource.scope) {
        (ResourceKind::Skills, ResourceScope::User) if linked_user_skill => "user-skill-link-v1",
        (ResourceKind::Skills, ResourceScope::User) => "user-skill-artifact-v1",
        (ResourceKind::Skills, ResourceScope::Project) => "project-skill-link-v1",
        (ResourceKind::Plugins, _) if resource.installation_id.as_str().starts_with("codex:") => {
            "codex-plugin-link-v1"
        }
        (ResourceKind::Plugins, _) => "claude-plugin-link-v1",
        _ => unreachable!("catalog ownership only handles collections"),
    };
    let user_artifact_root = if adapter_contract == "user-skill-artifact-v1" {
        Some(
            user_artifact_root(artifact_id, &candidate.subpath).ok_or_else(|| {
                ownership_error(
                    AgentErrorCode::InvalidPlan,
                    resource,
                    "User Skill artifact path does not match its catalog subpath",
                )
            })?,
        )
    } else {
        None
    };
    Ok(Some(CatalogOwnershipBinding {
        resource_id: candidate.id.clone(),
        source_id: source_id.to_owned(),
        install_id: install_id.to_owned(),
        resource_kind: resource.kind,
        stable_root: user_artifact_root
            .map(|root| root.to_string_lossy().into_owned())
            .unwrap_or_else(|| binding.stable_root.clone()),
        resource_subpath: candidate.subpath.clone(),
        adapter_contract: adapter_contract.into(),
    }))
}

fn validate_owned_catalog_binding(
    record: &ResourceOwnershipRecord,
    owned: &CatalogOwnershipBinding,
) -> Result<(), AgentError> {
    if owned.adapter_contract == "user-skill-artifact-v1" {
        let artifact = Path::new(&record.artifact_id);
        let artifacts_root = crate::fs::paths::skill_artifacts_dir().map_err(|error| {
            ownership_error(AgentErrorCode::Io, &record.resource, error.to_string())
        })?;
        let digest = super::directory_tree_digest(artifact).map_err(|error| {
            ownership_error(
                AgentErrorCode::ResourceChanged,
                &record.resource,
                error.to_string(),
            )
        })?;
        if record.resource.kind == ResourceKind::Skills
            && record.resource.scope == ResourceScope::User
            && artifact.starts_with(artifacts_root)
            && owned.source_id
                == record
                    .resource
                    .logical_id
                    .rsplit_once('/')
                    .map(|(source, _)| source)
                    .unwrap_or_default()
            && owned.install_id
                == record
                    .resource
                    .logical_id
                    .rsplit_once('/')
                    .map(|(_, install)| install)
                    .unwrap_or_default()
            && Path::new(&owned.stable_root).join(&owned.resource_subpath) == artifact
            && digest == record.artifact_digest
        {
            return Ok(());
        }
        return Err(ownership_error(
            AgentErrorCode::PermissionDenied,
            &record.resource,
            "Owned user Skill artifact identity changed",
        ));
    }
    let resolved = super::resolve_catalog_resource(&owned.resource_id).map_err(|error| {
        ownership_error(
            AgentErrorCode::ResourceChanged,
            &record.resource,
            error.to_string(),
        )
    })?;
    if resolved.source_id != owned.source_id
        || resolved.install_id != owned.install_id
        || resolved.kind != owned.resource_kind
        || resolved.stable_path != Path::new(&record.artifact_id)
    {
        return Err(ownership_error(
            AgentErrorCode::PermissionDenied,
            &record.resource,
            "Owned catalog resource identity changed",
        ));
    }
    Ok(())
}

fn user_artifact_root(artifact_id: &str, subpath: &str) -> Option<PathBuf> {
    let mut root = PathBuf::from(artifact_id);
    for component in Path::new(subpath).components().rev() {
        let Component::Normal(expected) = component else {
            return None;
        };
        if root.file_name()? != expected || !root.pop() {
            return None;
        }
    }
    (root.join(subpath) == Path::new(artifact_id)).then_some(root)
}

fn validate_owned_skill_binding(
    record: &ResourceOwnershipRecord,
    owned: &SkillOwnershipBinding,
) -> Result<(), AgentError> {
    let catalog = load_skill_catalog_snapshot().map_err(|error| {
        ownership_error(
            AgentErrorCode::ResourceChanged,
            &record.resource,
            format!("Skill catalog is unavailable: {error}"),
        )
    })?;
    let entry = catalog
        .entries
        .iter()
        .find(|entry| entry.source_id == owned.source_id)
        .ok_or_else(|| {
            ownership_error(
                AgentErrorCode::ResourceChanged,
                &record.resource,
                "Owned Skill source is no longer cataloged",
            )
        })?;
    let binding = entry.current_binding.as_ref().ok_or_else(|| {
        ownership_error(
            AgentErrorCode::ResourceChanged,
            &record.resource,
            "Owned Skill source is not a live source binding",
        )
    })?;
    if binding.binding_id != owned.binding_id
        || binding.source_id != owned.source_id
        || binding.source_type != owned.source_type
        || binding.stable_root != owned.stable_root
    {
        return Err(ownership_error(
            AgentErrorCode::PermissionDenied,
            &record.resource,
            "Owned Skill source identity changed",
        ));
    }
    let item = binding
        .skills
        .iter()
        .find(|item| item.subpath == owned.skill_subpath)
        .ok_or_else(|| {
            ownership_error(
                AgentErrorCode::ResourceChanged,
                &record.resource,
                "Owned Skill subpath is unavailable",
            )
        })?;
    let (stable_skill, _) = resolve_skill_source_item(binding, item).map_err(|error| {
        ownership_error(
            AgentErrorCode::ResourceChanged,
            &record.resource,
            error.to_string(),
        )
    })?;
    if stable_skill != Path::new(&record.artifact_id) {
        return Err(ownership_error(
            AgentErrorCode::PermissionDenied,
            &record.resource,
            "Owned Skill link target changed identity",
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
    let valid_schema = matches!(
        record.schema_version,
        LEGACY_RESOURCE_OWNERSHIP_SCHEMA_VERSION
            | SKILL_RESOURCE_OWNERSHIP_SCHEMA_VERSION
            | RESOURCE_OWNERSHIP_SCHEMA_VERSION
    ) && (record.schema_version != LEGACY_RESOURCE_OWNERSHIP_SCHEMA_VERSION
        || (record.source_binding.is_none() && record.catalog_binding.is_none()))
        && (record.schema_version != SKILL_RESOURCE_OWNERSHIP_SCHEMA_VERSION
            || record.catalog_binding.is_none());
    let valid_identity = valid_schema
        && &record.id == expected_id
        && record.target_id == PhysicalTargetId::for_resource(&record.resource)
        && record.workspace_key == ownership_workspace_key(&record.resource)?
        && match record.resource.scope {
            ResourceScope::User => record.resource.project_path.is_none(),
            ResourceScope::Project => record.resource.project_path.is_some(),
        };
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
