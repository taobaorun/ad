use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};

use crate::fs::atomic::write_atomic;
use crate::fs::paths::{backups_dir, ensure_dir, history_dir};

use super::execution_fs::{
    observe_target, remove_target, render_content, write_symlink_atomic, TargetState,
};
use super::{
    builtin_registry, AgentContext, AgentError, AgentErrorCode, AppliedResourceState,
    ContentDigest, ManagedResourceTarget, MutationKind, MutationPlan, OperationReceipt,
    OperationStatus, PlanId, PlanStore, PlannedMutation, ReceiptId, ResourceRef, ResourceStateKind,
    ResourceStorage,
};

static EXECUTION_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Default)]
pub struct ExecutionEngine;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum ExecutionStep {
    Backup(usize),
    Apply(usize),
    Compensate(usize),
}

trait FaultInjector {
    fn should_fail(&self, step: ExecutionStep) -> bool;
}

struct NoFaults;

impl FaultInjector for NoFaults {
    fn should_fail(&self, _step: ExecutionStep) -> bool {
        false
    }
}

#[cfg(test)]
pub(crate) struct FailAt {
    steps: HashSet<ExecutionStep>,
}

#[cfg(test)]
impl FailAt {
    pub(crate) fn new(steps: impl IntoIterator<Item = ExecutionStep>) -> Self {
        Self {
            steps: steps.into_iter().collect(),
        }
    }
}

#[cfg(test)]
impl FaultInjector for FailAt {
    fn should_fail(&self, step: ExecutionStep) -> bool {
        self.steps.contains(&step)
    }
}

struct ResolvedMutation {
    mutation: PlannedMutation,
    target: ManagedResourceTarget,
    original: TargetState,
    backup_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BackupManifest {
    plan_id: PlanId,
    entries: Vec<BackupManifestEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BackupManifestEntry {
    resource: ResourceRef,
    target_path: String,
    original_kind: String,
    backup_path: Option<String>,
    link_target: Option<String>,
}

impl ExecutionEngine {
    pub fn apply(
        &self,
        plan_id: &PlanId,
        plans: &PlanStore,
    ) -> Result<OperationReceipt, AgentError> {
        self.apply_internal(plan_id, plans, &NoFaults)
    }

    pub fn apply_confirmed(
        &self,
        plan_id: &PlanId,
        plans: &PlanStore,
    ) -> Result<OperationReceipt, AgentError> {
        self.apply_internal_with_confirmation(plan_id, plans, &NoFaults, true)
    }

    pub fn rollback(&self, receipt_id: &ReceiptId) -> Result<OperationReceipt, AgentError> {
        let _guard = EXECUTION_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let registry = builtin_registry();
        let plan = rollback_plan(receipt_id, &registry)?;
        execute_plan(plan, &registry, &NoFaults)
    }

    #[cfg(test)]
    pub(crate) fn apply_with_faults(
        &self,
        plan_id: &PlanId,
        plans: &PlanStore,
        faults: &FailAt,
    ) -> Result<OperationReceipt, AgentError> {
        self.apply_internal(plan_id, plans, faults)
    }

    fn apply_internal(
        &self,
        plan_id: &PlanId,
        plans: &PlanStore,
        faults: &dyn FaultInjector,
    ) -> Result<OperationReceipt, AgentError> {
        self.apply_internal_with_confirmation(plan_id, plans, faults, false)
    }

    fn apply_internal_with_confirmation(
        &self,
        plan_id: &PlanId,
        plans: &PlanStore,
        faults: &dyn FaultInjector,
        confirmed: bool,
    ) -> Result<OperationReceipt, AgentError> {
        let _guard = EXECUTION_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let registry = builtin_registry();
        let observe = |resource: &ResourceRef| {
            let context = AgentContext {
                installation_id: resource.installation_id.clone(),
                project_path: resource.project_path.clone(),
            };
            let target = registry.resolve_resource(&context, resource)?;
            Ok(observe_target(&target)?.digest())
        };
        let plan = if confirmed {
            plans.claim_confirmed(plan_id, observe)?
        } else {
            plans.claim_validated(plan_id, observe)?
        };
        execute_plan(plan, &registry, faults)
    }
}

fn rollback_plan(
    receipt_id: &ReceiptId,
    registry: &super::AdapterRegistry,
) -> Result<MutationPlan, AgentError> {
    uuid::Uuid::parse_str(receipt_id.as_str()).map_err(|_| {
        receipt_error(
            AgentErrorCode::InvalidPlan,
            receipt_id,
            None,
            "Invalid operation receipt id",
        )
    })?;
    let receipt_path = history_dir()
        .map_err(|error| receipt_error(AgentErrorCode::Io, receipt_id, None, error.to_string()))?
        .join("operations")
        .join(format!("{receipt_id}.json"));
    let receipt_bytes = std::fs::read(&receipt_path).map_err(|error| {
        receipt_error(
            AgentErrorCode::Io,
            receipt_id,
            None,
            format!("Failed to read {}: {error}", receipt_path.display()),
        )
    })?;
    let receipt: OperationReceipt = serde_json::from_slice(&receipt_bytes).map_err(|error| {
        receipt_error(
            AgentErrorCode::InvalidPlan,
            receipt_id,
            None,
            format!("Invalid operation receipt: {error}"),
        )
    })?;
    if receipt.status != OperationStatus::Complete {
        return Err(receipt_error(
            AgentErrorCode::InvalidPlan,
            receipt_id,
            None,
            "Only complete operations can be rolled back",
        ));
    }
    let expected_manifest_digest = receipt.manifest_digest.clone().ok_or_else(|| {
        receipt_error(
            AgentErrorCode::Unsupported,
            receipt_id,
            None,
            "Receipt predates digest-protected rollback",
        )
    })?;
    if receipt.post_apply_states.is_empty() {
        return Err(receipt_error(
            AgentErrorCode::Unsupported,
            receipt_id,
            None,
            "Receipt has no post-apply resource states",
        ));
    }

    let operation_dir = backups_dir()
        .map_err(|error| receipt_error(AgentErrorCode::Io, receipt_id, None, error.to_string()))?
        .join("operations")
        .join(receipt_id.as_str());
    let manifest_path = operation_dir.join("manifest.json");
    let manifest_bytes = std::fs::read(&manifest_path).map_err(|error| {
        receipt_error(
            AgentErrorCode::Io,
            receipt_id,
            None,
            format!("Failed to read {}: {error}", manifest_path.display()),
        )
    })?;
    if ContentDigest::sha256(&manifest_bytes) != expected_manifest_digest {
        return Err(receipt_error(
            AgentErrorCode::ResourceChanged,
            receipt_id,
            None,
            "Backup manifest changed after apply",
        ));
    }
    let manifest: BackupManifest = serde_json::from_slice(&manifest_bytes).map_err(|error| {
        receipt_error(
            AgentErrorCode::InvalidPlan,
            receipt_id,
            None,
            format!("Invalid backup manifest: {error}"),
        )
    })?;
    if manifest.plan_id != receipt.plan_id {
        return Err(receipt_error(
            AgentErrorCode::InvalidPlan,
            receipt_id,
            None,
            "Receipt and backup manifest plan ids differ",
        ));
    }

    let applied_resources = receipt
        .applied_resources
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    let state_count = receipt.post_apply_states.len();
    let mut states = receipt
        .post_apply_states
        .into_iter()
        .map(|state| (state.resource.clone(), state))
        .collect::<BTreeMap<_, _>>();
    if states.len() != state_count {
        return Err(receipt_error(
            AgentErrorCode::InvalidPlan,
            receipt_id,
            None,
            "Receipt contains duplicate resource states",
        ));
    }
    if applied_resources.len() != receipt.applied_resources.len()
        || applied_resources != states.keys().cloned().collect()
    {
        return Err(receipt_error(
            AgentErrorCode::InvalidPlan,
            receipt_id,
            None,
            "Receipt applied resources do not match post-apply states",
        ));
    }
    let first = states.values().next().ok_or_else(|| {
        receipt_error(
            AgentErrorCode::InvalidPlan,
            receipt_id,
            None,
            "Receipt has no applied resources",
        )
    })?;
    let installation_id = first.resource.installation_id.clone();
    let project_path = states
        .values()
        .filter_map(|state| state.resource.project_path.clone())
        .next();
    let context = AgentContext {
        installation_id: installation_id.clone(),
        project_path,
    };
    let installation = registry
        .discover()
        .into_iter()
        .find(|installation| installation.id == installation_id)
        .ok_or_else(|| {
            receipt_error(
                AgentErrorCode::Unsupported,
                receipt_id,
                None,
                "Receipt Agent installation is no longer available",
            )
        })?;

    let mut mutations = Vec::new();
    for entry in manifest.entries {
        let state = states.remove(&entry.resource).ok_or_else(|| {
            receipt_error(
                AgentErrorCode::InvalidPlan,
                receipt_id,
                Some(entry.resource.clone()),
                "Receipt resources do not match the backup manifest",
            )
        })?;
        if entry.resource.installation_id != context.installation_id
            || entry.resource.project_path.is_some()
                && entry.resource.project_path != context.project_path
        {
            return Err(receipt_error(
                AgentErrorCode::InvalidPlan,
                receipt_id,
                Some(entry.resource),
                "Rollback resources do not share one Agent context",
            ));
        }
        let resource_context = AgentContext {
            installation_id: entry.resource.installation_id.clone(),
            project_path: entry.resource.project_path.clone(),
        };
        let target = registry.resolve_resource(&resource_context, &entry.resource)?;
        let current = observe_target(&target)?;
        if current.kind() != state.kind || current.digest() != state.digest {
            return Err(receipt_error(
                AgentErrorCode::ResourceChanged,
                receipt_id,
                Some(entry.resource),
                "Target changed after apply; rollback refused",
            ));
        }
        mutations.push(rollback_mutation(
            receipt_id,
            &operation_dir,
            entry,
            state.kind,
            state.digest,
        )?);
    }
    if !states.is_empty() || mutations.is_empty() {
        return Err(receipt_error(
            AgentErrorCode::InvalidPlan,
            receipt_id,
            None,
            "Receipt resources do not match the backup manifest",
        ));
    }
    let plan = MutationPlan {
        id: PlanId::from(uuid::Uuid::new_v4().to_string()),
        agent_id: installation.agent_id,
        context,
        read_set: Vec::new(),
        mutations,
        expires_at: Utc::now() + Duration::minutes(5),
    };
    plan.validate()?;
    Ok(plan)
}

fn rollback_mutation(
    receipt_id: &ReceiptId,
    operation_dir: &Path,
    entry: BackupManifestEntry,
    current_kind: ResourceStateKind,
    expected_digest: Option<ContentDigest>,
) -> Result<PlannedMutation, AgentError> {
    let resource = entry.resource;
    match entry.original_kind.as_str() {
        "missing" => Ok(PlannedMutation {
            resource,
            kind: MutationKind::Delete,
            expected_digest,
            media_type: "application/octet-stream".into(),
            content: None,
        }),
        "file" => {
            let backup_path = entry.backup_path.ok_or_else(|| {
                receipt_error(
                    AgentErrorCode::InvalidPlan,
                    receipt_id,
                    Some(resource.clone()),
                    "File rollback entry has no backup",
                )
            })?;
            let bytes = read_contained_backup(receipt_id, operation_dir, Path::new(&backup_path))?;
            let content = String::from_utf8(bytes).map_err(|error| {
                receipt_error(
                    AgentErrorCode::Unsupported,
                    receipt_id,
                    Some(resource.clone()),
                    format!("Backup is not UTF-8: {error}"),
                )
            })?;
            Ok(PlannedMutation {
                resource,
                kind: if current_kind == ResourceStateKind::Missing {
                    MutationKind::Create
                } else {
                    MutationKind::Replace
                },
                expected_digest,
                media_type: "text/plain".into(),
                content: Some(serde_json::Value::String(content)),
            })
        }
        "symlink" => {
            let link_target = entry.link_target.ok_or_else(|| {
                receipt_error(
                    AgentErrorCode::InvalidPlan,
                    receipt_id,
                    Some(resource.clone()),
                    "Symlink rollback entry has no target",
                )
            })?;
            Ok(PlannedMutation {
                resource,
                kind: if current_kind == ResourceStateKind::Missing {
                    MutationKind::Create
                } else {
                    MutationKind::Replace
                },
                expected_digest,
                media_type: "application/vnd.ad.symlink".into(),
                content: Some(serde_json::Value::String(link_target)),
            })
        }
        _ => Err(receipt_error(
            AgentErrorCode::InvalidPlan,
            receipt_id,
            Some(resource),
            "Unknown backup manifest entry kind",
        )),
    }
}

fn read_contained_backup(
    receipt_id: &ReceiptId,
    operation_dir: &Path,
    backup_path: &Path,
) -> Result<Vec<u8>, AgentError> {
    let canonical_dir = std::fs::canonicalize(operation_dir)
        .map_err(|error| receipt_error(AgentErrorCode::Io, receipt_id, None, error.to_string()))?;
    let canonical_backup = std::fs::canonicalize(backup_path)
        .map_err(|error| receipt_error(AgentErrorCode::Io, receipt_id, None, error.to_string()))?;
    if !canonical_backup.starts_with(&canonical_dir) {
        return Err(receipt_error(
            AgentErrorCode::PermissionDenied,
            receipt_id,
            None,
            "Backup path escapes its operation directory",
        ));
    }
    std::fs::read(&canonical_backup)
        .map_err(|error| receipt_error(AgentErrorCode::Io, receipt_id, None, error.to_string()))
}

fn receipt_error(
    code: AgentErrorCode,
    receipt_id: &ReceiptId,
    resource: Option<ResourceRef>,
    message: impl Into<String>,
) -> AgentError {
    AgentError {
        code,
        message: message.into(),
        agent_id: None,
        installation_id: resource
            .as_ref()
            .map(|resource| resource.installation_id.clone()),
        resource,
        retryable: code == AgentErrorCode::ResourceChanged,
        details: Some(serde_json::json!({"receiptId": receipt_id})),
    }
}

fn execute_plan(
    plan: MutationPlan,
    registry: &super::AdapterRegistry,
    faults: &dyn FaultInjector,
) -> Result<OperationReceipt, AgentError> {
    let receipt_id = ReceiptId::from(uuid::Uuid::new_v4().to_string());
    let operation_dir = backups_dir()
        .map_err(|error| io_error(&plan, None, error.to_string()))?
        .join("operations")
        .join(receipt_id.as_str());
    ensure_dir(&operation_dir).map_err(|error| io_error(&plan, None, error.to_string()))?;

    let mut seen_targets = HashSet::new();
    let mut resolved = Vec::new();
    for (index, mutation) in plan.mutations.iter().cloned().enumerate() {
        let target = registry.resolve_resource(&plan.context, &mutation.resource)?;
        if !seen_targets.insert(target.path().to_path_buf()) {
            return Err(plan_error(
                &plan,
                Some(mutation.resource),
                "Multiple mutations resolve to the same physical target",
            ));
        }
        let original = observe_target(&target)?;
        ensure_storage_compatible(&plan, &mutation.resource, target.storage(), &original)?;
        ensure_expected(&plan, &mutation, original.digest().as_ref())?;
        fail_if_requested(&plan, faults, ExecutionStep::Backup(index))?;
        let backup_path = match &original {
            TargetState::File(bytes) => {
                let path = operation_dir.join(format!("{index}.backup"));
                write_atomic(&path, bytes).map_err(|error| {
                    io_error(&plan, Some(mutation.resource.clone()), error.to_string())
                })?;
                Some(path)
            }
            TargetState::Missing | TargetState::Symlink(_) => None,
        };
        resolved.push(ResolvedMutation {
            mutation,
            target,
            original,
            backup_path,
        });
    }

    let manifest = BackupManifest {
        plan_id: plan.id.clone(),
        entries: resolved.iter().map(manifest_entry).collect(),
    };
    let manifest_bytes = serde_json::to_vec_pretty(&manifest)
        .map_err(|error| io_error(&plan, None, error.to_string()))?;
    write_atomic(&operation_dir.join("manifest.json"), &manifest_bytes)
        .map_err(|error| io_error(&plan, None, error.to_string()))?;
    let manifest_digest = ContentDigest::sha256(&manifest_bytes);

    let mut applied = Vec::new();
    for (index, item) in resolved.iter().enumerate() {
        let current = observe_target(&item.target)?;
        if current.kind() != item.original.kind() || current.digest() != item.original.digest() {
            let error = changed_error(&plan, &item.mutation.resource);
            return finish_failed(
                &plan,
                receipt_id,
                &resolved,
                &applied,
                error,
                faults,
                manifest_digest,
            );
        }
        let apply_result = fail_if_requested(&plan, faults, ExecutionStep::Apply(index))
            .and_then(|()| apply_mutation(&plan, item));
        if let Err(error) = apply_result {
            return finish_failed(
                &plan,
                receipt_id,
                &resolved,
                &applied,
                error,
                faults,
                manifest_digest,
            );
        }
        applied.push(index);
    }

    let receipt = receipt(
        &plan,
        receipt_id,
        OperationStatus::Complete,
        &resolved,
        &applied,
        Some(manifest_digest),
        None,
    )?;
    persist_receipt(&plan, &receipt)?;
    Ok(receipt)
}

fn finish_failed(
    plan: &MutationPlan,
    receipt_id: ReceiptId,
    resolved: &[ResolvedMutation],
    applied: &[usize],
    cause: AgentError,
    faults: &dyn FaultInjector,
    manifest_digest: ContentDigest,
) -> Result<OperationReceipt, AgentError> {
    if applied.is_empty() {
        return Err(cause);
    }
    let mut compensation_errors = Vec::new();
    for index in applied.iter().rev() {
        let restore_result = fail_if_requested(plan, faults, ExecutionStep::Compensate(*index))
            .and_then(|()| restore_target(plan, &resolved[*index]));
        if let Err(error) = restore_result {
            compensation_errors.push(error.message);
        }
    }
    let (status, message) = if compensation_errors.is_empty() {
        (
            OperationStatus::Compensated,
            format!(
                "Execution failed and applied changes were compensated: {}",
                cause.message
            ),
        )
    } else {
        (
            OperationStatus::PartialFailure,
            format!(
                "Execution failed: {}; compensation also failed: {}",
                cause.message,
                compensation_errors.join("; ")
            ),
        )
    };
    let receipt = receipt(
        plan,
        receipt_id,
        status,
        resolved,
        applied,
        Some(manifest_digest),
        Some(message),
    )?;
    persist_receipt(plan, &receipt)?;
    Ok(receipt)
}

fn fail_if_requested(
    plan: &MutationPlan,
    faults: &dyn FaultInjector,
    step: ExecutionStep,
) -> Result<(), AgentError> {
    if faults.should_fail(step) {
        Err(io_error(
            plan,
            None,
            format!("Injected execution failure at {step:?}"),
        ))
    } else {
        Ok(())
    }
}

fn ensure_expected(
    plan: &MutationPlan,
    mutation: &PlannedMutation,
    actual: Option<&ContentDigest>,
) -> Result<(), AgentError> {
    if mutation.expected_digest.as_ref() == actual {
        Ok(())
    } else {
        Err(changed_error(plan, &mutation.resource))
    }
}

fn ensure_storage_compatible(
    plan: &MutationPlan,
    resource: &ResourceRef,
    storage: ResourceStorage,
    state: &TargetState,
) -> Result<(), AgentError> {
    let compatible = matches!(state, TargetState::Missing)
        || matches!(
            (storage, state),
            (ResourceStorage::File, TargetState::File(_))
        )
        || matches!(
            (storage, state),
            (ResourceStorage::Symlink, TargetState::Symlink(_))
        );
    if compatible {
        Ok(())
    } else {
        Err(AgentError {
            code: AgentErrorCode::ResourceChanged,
            message: format!("Resource {} changed storage type", resource.logical_id),
            agent_id: Some(plan.agent_id.clone()),
            installation_id: Some(resource.installation_id.clone()),
            resource: Some(resource.clone()),
            retryable: true,
            details: None,
        })
    }
}

fn apply_mutation(plan: &MutationPlan, item: &ResolvedMutation) -> Result<(), AgentError> {
    match item.mutation.kind {
        MutationKind::Delete => remove_target(item.target.path()),
        MutationKind::Create | MutationKind::Replace => match item.target.storage() {
            ResourceStorage::File => {
                let bytes = render_content(&item.mutation)?;
                write_atomic(item.target.path(), &bytes).map_err(|error| {
                    io_error(
                        plan,
                        Some(item.mutation.resource.clone()),
                        error.to_string(),
                    )
                })
            }
            ResourceStorage::Symlink => {
                let source = item
                    .mutation
                    .content
                    .as_ref()
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| {
                        plan_error(
                            plan,
                            Some(item.mutation.resource.clone()),
                            "Symlink mutation requires a string target",
                        )
                    })?;
                write_symlink_atomic(item.target.path(), Path::new(source)).map_err(|error| {
                    io_error(
                        plan,
                        Some(item.mutation.resource.clone()),
                        error.to_string(),
                    )
                })
            }
        },
    }
}

fn restore_target(plan: &MutationPlan, item: &ResolvedMutation) -> Result<(), AgentError> {
    match &item.original {
        TargetState::Missing => remove_target(item.target.path()),
        TargetState::File(_) => {
            let backup = item.backup_path.as_ref().ok_or_else(|| {
                io_error(
                    plan,
                    Some(item.mutation.resource.clone()),
                    "Missing backup file",
                )
            })?;
            let bytes = std::fs::read(backup).map_err(|error| {
                io_error(
                    plan,
                    Some(item.mutation.resource.clone()),
                    error.to_string(),
                )
            })?;
            write_atomic(item.target.path(), &bytes).map_err(|error| {
                io_error(
                    plan,
                    Some(item.mutation.resource.clone()),
                    error.to_string(),
                )
            })
        }
        TargetState::Symlink(source) => {
            write_symlink_atomic(item.target.path(), source).map_err(|error| {
                io_error(
                    plan,
                    Some(item.mutation.resource.clone()),
                    error.to_string(),
                )
            })
        }
    }
}

fn manifest_entry(item: &ResolvedMutation) -> BackupManifestEntry {
    let (original_kind, link_target) = match &item.original {
        TargetState::Missing => ("missing".into(), None),
        TargetState::File(_) => ("file".into(), None),
        TargetState::Symlink(target) => (
            "symlink".into(),
            Some(target.to_string_lossy().into_owned()),
        ),
    };
    BackupManifestEntry {
        resource: item.mutation.resource.clone(),
        target_path: item.target.path().to_string_lossy().into_owned(),
        original_kind,
        backup_path: item
            .backup_path
            .as_ref()
            .map(|path| path.to_string_lossy().into_owned()),
        link_target,
    }
}

fn receipt(
    plan: &MutationPlan,
    id: ReceiptId,
    status: OperationStatus,
    resolved: &[ResolvedMutation],
    applied: &[usize],
    manifest_digest: Option<ContentDigest>,
    message: Option<String>,
) -> Result<OperationReceipt, AgentError> {
    let post_apply_states = applied
        .iter()
        .map(|index| {
            let item = &resolved[*index];
            let state = observe_target(&item.target)?;
            Ok(AppliedResourceState {
                resource: item.mutation.resource.clone(),
                kind: state.kind(),
                digest: state.digest(),
            })
        })
        .collect::<Result<Vec<_>, AgentError>>()?;
    Ok(OperationReceipt {
        id,
        plan_id: plan.id.clone(),
        status,
        applied_resources: applied
            .iter()
            .map(|index| resolved[*index].mutation.resource.clone())
            .collect(),
        backup_paths: resolved
            .iter()
            .filter_map(|item| item.backup_path.as_ref())
            .map(|path| path.to_string_lossy().into_owned())
            .collect(),
        post_apply_states,
        manifest_digest,
        message,
    })
}

fn persist_receipt(plan: &MutationPlan, receipt: &OperationReceipt) -> Result<(), AgentError> {
    let directory = history_dir()
        .map_err(|error| io_error(plan, None, error.to_string()))?
        .join("operations");
    ensure_dir(&directory).map_err(|error| io_error(plan, None, error.to_string()))?;
    let bytes = serde_json::to_vec_pretty(receipt)
        .map_err(|error| io_error(plan, None, error.to_string()))?;
    write_atomic(&directory.join(format!("{}.json", receipt.id)), &bytes)
        .map_err(|error| io_error(plan, None, error.to_string()))
}

fn plan_error(
    plan: &MutationPlan,
    resource: Option<ResourceRef>,
    message: impl Into<String>,
) -> AgentError {
    AgentError {
        code: AgentErrorCode::InvalidPlan,
        message: message.into(),
        agent_id: Some(plan.agent_id.clone()),
        installation_id: Some(plan.context.installation_id.clone()),
        resource,
        retryable: false,
        details: None,
    }
}

fn changed_error(plan: &MutationPlan, resource: &ResourceRef) -> AgentError {
    AgentError {
        code: AgentErrorCode::ResourceChanged,
        message: format!("Resource {} changed during execution", resource.logical_id),
        agent_id: Some(plan.agent_id.clone()),
        installation_id: Some(plan.context.installation_id.clone()),
        resource: Some(resource.clone()),
        retryable: true,
        details: None,
    }
}

fn io_error(
    plan: &MutationPlan,
    resource: Option<ResourceRef>,
    message: impl Into<String>,
) -> AgentError {
    AgentError {
        code: AgentErrorCode::Io,
        message: message.into(),
        agent_id: Some(plan.agent_id.clone()),
        installation_id: Some(plan.context.installation_id.clone()),
        resource,
        retryable: true,
        details: None,
    }
}
