use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::Serialize;

use crate::fs::atomic::write_atomic;
use crate::fs::paths::{backups_dir, ensure_dir, history_dir};

use super::execution_fs::{
    observe_target, remove_target, render_content, write_symlink_atomic, TargetState,
};
use super::{
    builtin_registry, AgentContext, AgentError, AgentErrorCode, ContentDigest,
    ManagedResourceTarget, MutationKind, MutationPlan, OperationReceipt, OperationStatus, PlanId,
    PlanStore, PlannedMutation, ReceiptId, ResourceRef, ResourceStorage,
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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BackupManifest<'a> {
    plan_id: &'a PlanId,
    entries: Vec<BackupManifestEntry>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BackupManifestEntry {
    resource: ResourceRef,
    target_path: String,
    original_kind: &'static str,
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
        let _guard = EXECUTION_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let registry = builtin_registry();
        let plan = plans.claim_validated(plan_id, |resource| {
            let context = AgentContext {
                installation_id: resource.installation_id.clone(),
                project_path: resource.project_path.clone(),
            };
            let target = registry.resolve_resource(&context, resource)?;
            Ok(observe_target(&target)?.digest())
        })?;
        execute_plan(plan, &registry, faults)
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
        plan_id: &plan.id,
        entries: resolved.iter().map(manifest_entry).collect(),
    };
    let manifest_bytes = serde_json::to_vec_pretty(&manifest)
        .map_err(|error| io_error(&plan, None, error.to_string()))?;
    write_atomic(&operation_dir.join("manifest.json"), &manifest_bytes)
        .map_err(|error| io_error(&plan, None, error.to_string()))?;

    let mut applied = Vec::new();
    for (index, item) in resolved.iter().enumerate() {
        let current = observe_target(&item.target)?;
        if current.digest() != item.original.digest() {
            let error = changed_error(&plan, &item.mutation.resource);
            return finish_failed(&plan, receipt_id, &resolved, &applied, error, faults);
        }
        let apply_result = fail_if_requested(&plan, faults, ExecutionStep::Apply(index))
            .and_then(|()| apply_mutation(&plan, item));
        if let Err(error) = apply_result {
            return finish_failed(&plan, receipt_id, &resolved, &applied, error, faults);
        }
        applied.push(index);
    }

    let receipt = receipt(
        &plan,
        receipt_id,
        OperationStatus::Complete,
        &resolved,
        &applied,
        None,
    );
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
    let receipt = receipt(plan, receipt_id, status, resolved, applied, Some(message));
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
        TargetState::Missing => ("missing", None),
        TargetState::File(_) => ("file", None),
        TargetState::Symlink(target) => ("symlink", Some(target.to_string_lossy().into_owned())),
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
    message: Option<String>,
) -> OperationReceipt {
    OperationReceipt {
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
        message,
    }
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
