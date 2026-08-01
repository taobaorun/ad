use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};

use super::execution_confinement::ConfinedTarget;
use super::execution_fs::{directory_tree_digest, render_content, TargetState};
use super::execution_recovery::{mark_repaired, MutationRecoveryLease};
use super::execution_state::{ExecutionState, StateDirectory};
use super::{
    apply_ownership_changes, builtin_registry, decode_operation_receipt, execution_instance_id,
    load_ownership_record, ownership_managed, ownership_record_id, ownership_workspace_key,
    validate_ownership_artifact, validate_ownership_record, validate_ownership_record_identity,
    workspace_key_for_context, AgentContext, AgentError, AgentErrorCode, AppliedResourceState,
    ContentDigest, ManagedResourceTarget, MutationKind, MutationPlan, MutationPlanView,
    OperationJournalHandle, OperationJournalState, OperationKind, OperationReceipt,
    OperationStatus, OwnershipArtifact, OwnershipRestore, PhysicalTargetId, PlanAcknowledgement,
    PlanClaimBinding, PlanExecutionIntent, PlanId, PlanStore, PlannedMutation, ReceiptId,
    ResourceKind, ResourceOwnershipChange, ResourceOwnershipChangeKind, ResourceOwnershipRecord,
    ResourceRef, ResourceScope, ResourceStateKind, ResourceStorage, RiskFingerprint,
    RollbackEligibility, RollbackUnavailableReason, TargetLockSet, WritePolicy,
    OPERATION_RECEIPT_SCHEMA_VERSION, OWNERSHIP_EVIDENCE_VERSION,
    RESOURCE_OWNERSHIP_SCHEMA_VERSION,
};

static EXECUTION_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Default)]
pub struct ExecutionEngine;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum ExecutionStep {
    Backup(usize),
    Apply(usize),
    ApplyPublished(usize),
    Compensate(usize),
    ConstructReceipt,
    PersistReceipt,
    PersistReceiptPublished,
    PersistJournalApplying,
}

pub(crate) trait FaultInjector {
    fn before_step(&self, _step: ExecutionStep) {}

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
    confined_target: ConfinedTarget,
    directory_source: Option<StateDirectory>,
    original: TargetState,
    backup_name: Option<String>,
    backup_path: Option<PathBuf>,
    backup_digest: Option<ContentDigest>,
    ownership_before: Option<ResourceOwnershipRecord>,
    ownership_artifact: Option<OwnershipArtifact>,
}

struct FailureContext<'a> {
    resolved: &'a [ResolvedMutation],
    applied: &'a [usize],
    state: &'a ExecutionState,
    operation_dir: &'a StateDirectory,
    operation_name: &'a str,
    faults: &'a dyn FaultInjector,
    journal: &'a mut OperationJournalHandle,
    intent: &'a PlanExecutionIntent,
}

struct ReceiptDraft<'a> {
    status: OperationStatus,
    resolved: &'a [ResolvedMutation],
    applied: &'a [usize],
    manifest_digest: Option<ContentDigest>,
    message: Option<String>,
    intent: &'a PlanExecutionIntent,
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
    #[serde(default)]
    backup_digest: Option<ContentDigest>,
    link_target: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    ownership_before: Option<ResourceOwnershipRecord>,
}

struct InverseMutationPlan {
    plan: MutationPlan,
    ownership_restores: BTreeMap<PhysicalTargetId, OwnershipRestore>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DirectoryMutationSource {
    path: String,
    digest: ContentDigest,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum SymlinkMutationSource {
    Legacy(String),
    Checked { path: String, digest: ContentDigest },
}

impl SymlinkMutationSource {
    fn path(&self) -> &str {
        match self {
            Self::Legacy(path) | Self::Checked { path, .. } => path,
        }
    }

    fn digest(&self) -> Option<&ContentDigest> {
        match self {
            Self::Legacy(_) => None,
            Self::Checked { digest, .. } => Some(digest),
        }
    }
}

impl ExecutionEngine {
    pub fn apply(
        &self,
        plan_id: &PlanId,
        plans: &PlanStore,
    ) -> Result<OperationReceipt, AgentError> {
        let binding = plans.claim_binding(plan_id)?;
        self.apply_internal(plan_id, plans, &binding, &NoFaults)
    }

    pub fn apply_bound(
        &self,
        plan_id: &PlanId,
        expected_context: &AgentContext,
        expected_risk_fingerprint: &RiskFingerprint,
        plans: &PlanStore,
    ) -> Result<OperationReceipt, AgentError> {
        self.apply_internal(
            plan_id,
            plans,
            &PlanClaimBinding {
                context: expected_context.clone(),
                risk_fingerprint: expected_risk_fingerprint.clone(),
            },
            &NoFaults,
        )
    }

    pub fn apply_confirmed(
        &self,
        plan_id: &PlanId,
        plans: &PlanStore,
    ) -> Result<OperationReceipt, AgentError> {
        let binding = plans.claim_binding(plan_id)?;
        self.apply_internal_with_confirmation(plan_id, plans, &binding, &NoFaults, true)
    }

    pub fn apply_acknowledged(
        &self,
        plan_id: &PlanId,
        plans: &PlanStore,
        acknowledgements: &[PlanAcknowledgement],
    ) -> Result<OperationReceipt, AgentError> {
        let binding = plans.claim_binding(plan_id)?;
        self.apply_internal_with_acknowledgements(
            plan_id,
            plans,
            &binding,
            &NoFaults,
            acknowledgements,
        )
    }

    pub fn apply_acknowledged_bound(
        &self,
        plan_id: &PlanId,
        expected_context: &AgentContext,
        expected_risk_fingerprint: &RiskFingerprint,
        plans: &PlanStore,
        acknowledgements: &[PlanAcknowledgement],
    ) -> Result<OperationReceipt, AgentError> {
        self.apply_internal_with_acknowledgements(
            plan_id,
            plans,
            &PlanClaimBinding {
                context: expected_context.clone(),
                risk_fingerprint: expected_risk_fingerprint.clone(),
            },
            &NoFaults,
            acknowledgements,
        )
    }

    pub fn preview_rollback(
        &self,
        receipt_id: &ReceiptId,
        plans: &PlanStore,
    ) -> Result<MutationPlanView, AgentError> {
        let _guard = EXECUTION_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let state = ExecutionState::open().map_err(execution_state_error)?;
        let _recovery_lease = MutationRecoveryLease::acquire_for_rollback(&state, receipt_id)?;
        let registry = builtin_registry();
        let inverse = rollback_plan(receipt_id, None, &registry, &state)?;
        plans.insert_rollback(inverse.plan, receipt_id.clone(), inverse.ownership_restores)
    }

    pub fn preview_rollback_bound(
        &self,
        receipt_id: &ReceiptId,
        expected_context: &AgentContext,
        plans: &PlanStore,
    ) -> Result<MutationPlanView, AgentError> {
        let _guard = EXECUTION_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let state = ExecutionState::open().map_err(execution_state_error)?;
        let _recovery_lease = MutationRecoveryLease::acquire_for_rollback(&state, receipt_id)?;
        let registry = builtin_registry();
        let inverse = rollback_plan(receipt_id, Some(expected_context), &registry, &state)?;
        plans.insert_rollback(inverse.plan, receipt_id.clone(), inverse.ownership_restores)
    }

    #[cfg(test)]
    pub(crate) fn apply_with_faults(
        &self,
        plan_id: &PlanId,
        plans: &PlanStore,
        faults: &dyn FaultInjector,
    ) -> Result<OperationReceipt, AgentError> {
        let binding = plans.claim_binding(plan_id)?;
        self.apply_internal(plan_id, plans, &binding, faults)
    }

    fn apply_internal(
        &self,
        plan_id: &PlanId,
        plans: &PlanStore,
        binding: &PlanClaimBinding,
        faults: &dyn FaultInjector,
    ) -> Result<OperationReceipt, AgentError> {
        self.apply_internal_with_confirmation(plan_id, plans, binding, faults, false)
    }

    fn apply_internal_with_confirmation(
        &self,
        plan_id: &PlanId,
        plans: &PlanStore,
        binding: &PlanClaimBinding,
        faults: &dyn FaultInjector,
        confirmed: bool,
    ) -> Result<OperationReceipt, AgentError> {
        let acknowledgements = confirmed.then_some(PlanAcknowledgement {
            code: super::PlanAcknowledgementCode::ConversionApply,
            accepted: true,
        });
        self.apply_internal_with_acknowledgements(
            plan_id,
            plans,
            binding,
            faults,
            acknowledgements.as_slice(),
        )
    }

    fn apply_internal_with_acknowledgements(
        &self,
        plan_id: &PlanId,
        plans: &PlanStore,
        binding: &PlanClaimBinding,
        faults: &dyn FaultInjector,
        acknowledgements: &[PlanAcknowledgement],
    ) -> Result<OperationReceipt, AgentError> {
        let _guard = EXECUTION_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let state = ExecutionState::open().map_err(execution_state_error)?;
        let intent = plans.execution_intent(plan_id)?;
        let _recovery_lease = match intent.parent_receipt_id.as_ref() {
            Some(receipt_id) => MutationRecoveryLease::acquire_for_rollback(&state, receipt_id)?,
            None => MutationRecoveryLease::acquire(&state)?,
        };
        let registry = builtin_registry();
        let resources = plans.resources_for_locking(plan_id)?;
        let _target_locks =
            TargetLockSet::acquire_for_resources(&resources, plan_id.as_str(), &registry, &state)?;
        let observe = |resource: &ResourceRef| {
            let context = AgentContext {
                installation_id: resource.installation_id.clone(),
                project_path: resource.project_path.clone(),
            };
            let target = registry.resolve_resource(&context, resource)?;
            Ok(ConfinedTarget::observe_dependency_bound(
                resource,
                target.path(),
                intent.project_root_identity,
            )?
            .digest())
        };
        let claimed =
            plans.claim_acknowledged_for_execution(plan_id, binding, acknowledgements, observe)?;
        let refresh_base_config = plan_refreshes_base_config(&claimed.plan);
        let receipt = execute_plan(claimed.plan, &claimed.intent, &registry, faults, &state)?;
        if receipt.status == OperationStatus::Complete {
            if let Some(parent_receipt_id) = claimed.intent.parent_receipt_id.as_ref() {
                mark_repaired(&state, parent_receipt_id)?;
            }
        }
        refresh_runtime_state(&receipt, refresh_base_config);
        Ok(receipt)
    }
}

fn plan_refreshes_base_config(plan: &MutationPlan) -> bool {
    plan.agent_id.as_str() == "codex"
        && plan.read_set.iter().any(|precondition| {
            precondition.resource.kind == ResourceKind::Plugins
                && precondition.resource.scope == ResourceScope::Project
                && precondition.resource.logical_id == "base-config"
                && precondition.write_policy == WritePolicy::ReadOnly
        })
}

fn refresh_runtime_state(receipt: &OperationReceipt, refresh_base_config: bool) {
    if receipt.status != OperationStatus::Complete {
        return;
    }
    let installation_id = receipt
        .applied_resources
        .first()
        .map(|resource| &resource.installation_id);
    let Some(installation_id) = installation_id else {
        return;
    };
    if super::runtime_for_installation(installation_id).is_none() {
        let project_path = receipt
            .applied_resources
            .iter()
            .find_map(|resource| resource.project_path.as_deref());
        if let Some(project_path) = project_path {
            match super::project_runtime_descriptor_for_context(
                installation_id,
                Path::new(project_path),
            ) {
                Ok(Some(runtime)) => {
                    if let Err(error) = super::persist_project_codex_runtime(&runtime) {
                        tracing::warn!(
                            installation_id = %installation_id,
                            error = %error,
                            "failed to register Project Codex runtime after execution"
                        );
                        return;
                    }
                }
                Ok(None) => {}
                Err(error) => {
                    tracing::warn!(
                        installation_id = %installation_id,
                        error = %error,
                        "failed to derive Project Codex runtime after execution"
                    );
                    return;
                }
            }
        }
    }
    if let Err(error) =
        super::refresh_project_codex_runtime_after_apply(installation_id, refresh_base_config)
    {
        tracing::warn!(
            installation_id = %installation_id,
            error = %error,
            "failed to refresh Project Codex runtime digests after execution"
        );
    }
}

fn rollback_plan(
    receipt_id: &ReceiptId,
    expected_context: Option<&AgentContext>,
    registry: &super::AdapterRegistry,
    execution_state: &ExecutionState,
) -> Result<InverseMutationPlan, AgentError> {
    uuid::Uuid::parse_str(receipt_id.as_str()).map_err(|_| {
        receipt_error(
            AgentErrorCode::InvalidPlan,
            receipt_id,
            None,
            "Invalid operation receipt id",
        )
    })?;
    let receipt_name = format!("{receipt_id}.json");
    let receipt_bytes = execution_state
        .history()
        .read(&receipt_name)
        .map_err(|error| {
            receipt_error(
                AgentErrorCode::Io,
                receipt_id,
                None,
                format!("Failed to read operation receipt: {error}"),
            )
        })?;
    let receipt = decode_operation_receipt(&receipt_bytes).map_err(|error| {
        receipt_error(
            AgentErrorCode::InvalidPlan,
            receipt_id,
            None,
            format!("Invalid operation receipt: {error}"),
        )
    })?;
    if expected_context.is_some() && receipt.context.as_ref() != expected_context {
        return Err(receipt_error(
            AgentErrorCode::ResourceChanged,
            receipt_id,
            None,
            "Operation receipt does not belong to the expected workspace context",
        ));
    }
    if receipt.operation_kind != OperationKind::Apply || receipt.parent_receipt_id.is_some() {
        return Err(receipt_error(
            AgentErrorCode::InvalidPlan,
            receipt_id,
            None,
            "Only root apply operation receipts can be rolled back",
        ));
    }
    if !receipt.rollback.available {
        return Err(receipt_error(
            AgentErrorCode::Unsupported,
            receipt_id,
            None,
            format!(
                "Operation receipt is not eligible for rollback: {:?}",
                receipt.rollback.reason
            ),
        ));
    }
    if !matches!(
        receipt.status,
        OperationStatus::Complete | OperationStatus::PartialFailure
    ) {
        return Err(receipt_error(
            AgentErrorCode::InvalidPlan,
            receipt_id,
            None,
            "Only complete or partially failed operations can be rolled back",
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

    let operation_dir = execution_state
        .backups()
        .open_directory(receipt_id.as_str())
        .map_err(|error| {
            receipt_error(
                AgentErrorCode::Io,
                receipt_id,
                None,
                format!("Failed to open operation backups: {error}"),
            )
        })?;
    let manifest_bytes = operation_dir.read("manifest.json").map_err(|error| {
        receipt_error(
            AgentErrorCode::Io,
            receipt_id,
            None,
            format!("Failed to read backup manifest: {error}"),
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
        .find(|installation| installation.id == installation_id);
    let agent_id = match installation {
        Some(installation) => installation.agent_id,
        None => {
            let derived_runtime = context
                .project_path
                .as_deref()
                .map(Path::new)
                .map(|project_path| {
                    super::project_runtime_descriptor_for_context(
                        &context.installation_id,
                        project_path,
                    )
                })
                .transpose()
                .map_err(|error| {
                    receipt_error(
                        AgentErrorCode::Unsupported,
                        receipt_id,
                        None,
                        error.to_string(),
                    )
                })?
                .flatten();
            if derived_runtime.is_some() {
                super::AgentId::from("codex")
            } else {
                return Err(receipt_error(
                    AgentErrorCode::Unsupported,
                    receipt_id,
                    None,
                    "Receipt Agent installation is no longer available",
                ));
            }
        }
    };

    let manifest_entry_count = manifest.entries.len();
    let mut manifest_entries = manifest
        .entries
        .into_iter()
        .map(|entry| (entry.resource.clone(), entry))
        .collect::<BTreeMap<_, _>>();
    if manifest_entries.len() != manifest_entry_count {
        return Err(receipt_error(
            AgentErrorCode::InvalidPlan,
            receipt_id,
            None,
            "Backup manifest contains duplicate resources",
        ));
    }
    if receipt.status == OperationStatus::Complete
        && manifest_entries.len() != applied_resources.len()
    {
        return Err(receipt_error(
            AgentErrorCode::InvalidPlan,
            receipt_id,
            None,
            "Complete receipt resources do not match the backup manifest",
        ));
    }

    let mut mutations = Vec::new();
    let ownership_change_count = receipt.ownership_changes.len();
    let mut ownership_changes = receipt
        .ownership_changes
        .into_iter()
        .map(|change| (change.record_id.clone(), change))
        .collect::<BTreeMap<_, _>>();
    if ownership_changes.len() != ownership_change_count {
        return Err(receipt_error(
            AgentErrorCode::InvalidPlan,
            receipt_id,
            None,
            "Receipt contains duplicate ownership changes",
        ));
    }
    let mut ownership_restores = BTreeMap::new();
    for resource in applied_resources {
        let entry = manifest_entries.remove(&resource).ok_or_else(|| {
            receipt_error(
                AgentErrorCode::InvalidPlan,
                receipt_id,
                Some(resource.clone()),
                "Receipt resources do not match the backup manifest",
            )
        })?;
        let state = states.remove(&resource).ok_or_else(|| {
            receipt_error(
                AgentErrorCode::InvalidPlan,
                receipt_id,
                Some(resource),
                "Receipt resources do not match their post-apply states",
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
        let current = ConfinedTarget::observe_existing(&entry.resource, target.path())?;
        if current.kind() != state.kind || current.digest() != state.digest {
            return Err(receipt_error(
                AgentErrorCode::ResourceChanged,
                receipt_id,
                Some(entry.resource),
                "Target changed after apply; rollback refused",
            ));
        }
        if ownership_managed(&entry.resource, target.storage()) {
            if receipt.ownership_evidence_version != OWNERSHIP_EVIDENCE_VERSION {
                return Err(receipt_error(
                    AgentErrorCode::Unsupported,
                    receipt_id,
                    Some(entry.resource.clone()),
                    "Receipt has no supported project resource ownership evidence",
                ));
            }
            let record_id = ownership_record_id(&entry.resource);
            let (expected_current, expected_matches_target) = match ownership_changes
                .remove(&record_id)
            {
                Some(change) if change.kind == ResourceOwnershipChangeKind::Upsert => (
                    Some(change.record.ok_or_else(|| {
                        receipt_error(
                            AgentErrorCode::InvalidPlan,
                            receipt_id,
                            Some(entry.resource.clone()),
                            "Ownership upsert receipt is missing its record",
                        )
                    })?),
                    true,
                ),
                Some(change) if change.kind == ResourceOwnershipChangeKind::Remove => (None, true),
                Some(_) => unreachable!("ownership change kinds are exhaustive"),
                None if receipt.status == OperationStatus::PartialFailure => {
                    (entry.ownership_before.clone(), false)
                }
                None => {
                    return Err(receipt_error(
                        AgentErrorCode::InvalidPlan,
                        receipt_id,
                        Some(entry.resource.clone()),
                        "Complete receipt is missing project resource ownership evidence",
                    ))
                }
            };
            let actual = load_ownership_record(execution_state, &entry.resource)?;
            if actual != expected_current {
                return Err(receipt_error(
                    AgentErrorCode::PermissionDenied,
                    receipt_id,
                    Some(entry.resource.clone()),
                    "Current ownership does not match the receipt",
                ));
            }
            if let Some(record) = expected_current.as_ref() {
                if expected_matches_target {
                    validate_ownership_record(
                        record,
                        &entry.resource,
                        target.path(),
                        current.kind(),
                        current.digest().as_ref(),
                    )?;
                }
                validate_ownership_artifact(record)?;
            }
            if let Some(record) = entry.ownership_before.as_ref() {
                validate_ownership_artifact(record)?;
            }
            ownership_restores.insert(
                PhysicalTargetId::for_resource(&entry.resource),
                OwnershipRestore {
                    expected_current,
                    expected_matches_target,
                    restore: entry.ownership_before.clone(),
                },
            );
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
    if !ownership_changes.is_empty() {
        return Err(receipt_error(
            AgentErrorCode::InvalidPlan,
            receipt_id,
            None,
            "Receipt ownership changes do not match its resources",
        ));
    }
    let plan = MutationPlan {
        id: PlanId::from(uuid::Uuid::new_v4().to_string()),
        agent_id,
        context,
        read_set: Vec::new(),
        mutations,
        expires_at: Utc::now() + Duration::minutes(5),
    };
    plan.validate()?;
    Ok(InverseMutationPlan {
        plan,
        ownership_restores,
    })
}

fn rollback_mutation(
    receipt_id: &ReceiptId,
    operation_dir: &StateDirectory,
    entry: BackupManifestEntry,
    current_kind: ResourceStateKind,
    expected_digest: Option<ContentDigest>,
) -> Result<PlannedMutation, AgentError> {
    let recorded_backup_digest = entry.backup_digest.clone();
    let ownership_before = entry.ownership_before.clone();
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
            let backup_name =
                contained_backup_name(receipt_id, operation_dir, Path::new(&backup_path))?;
            let bytes = operation_dir.read(&backup_name).map_err(|error| {
                receipt_error(AgentErrorCode::Io, receipt_id, None, error.to_string())
            })?;
            let expected_backup_digest = recorded_backup_digest.ok_or_else(|| {
                receipt_error(
                    AgentErrorCode::Unsupported,
                    receipt_id,
                    Some(resource.clone()),
                    "File backup predates digest-protected rollback",
                )
            })?;
            if ContentDigest::sha256(&bytes) != expected_backup_digest {
                return Err(receipt_error(
                    AgentErrorCode::ResourceChanged,
                    receipt_id,
                    Some(resource.clone()),
                    "File backup changed after apply",
                ));
            }
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
            let content = match ownership_before {
                Some(record) if record.artifact_id == link_target => serde_json::json!({
                    "path": link_target,
                    "digest": record.artifact_digest,
                }),
                Some(_) => {
                    return Err(receipt_error(
                        AgentErrorCode::InvalidPlan,
                        receipt_id,
                        Some(resource),
                        "Owned symlink artifact does not match its backup",
                    ))
                }
                None => serde_json::Value::String(link_target),
            };
            Ok(PlannedMutation {
                resource,
                kind: if current_kind == ResourceStateKind::Missing {
                    MutationKind::Create
                } else {
                    MutationKind::Replace
                },
                expected_digest,
                media_type: "application/vnd.ad.symlink".into(),
                content: Some(content),
            })
        }
        "directory" => {
            let backup_path = entry.backup_path.ok_or_else(|| {
                receipt_error(
                    AgentErrorCode::InvalidPlan,
                    receipt_id,
                    Some(resource.clone()),
                    "Directory rollback entry has no backup",
                )
            })?;
            let backup_name =
                contained_backup_name(receipt_id, operation_dir, Path::new(&backup_path))?;
            let actual_backup_digest =
                operation_dir
                    .directory_digest(&backup_name)
                    .map_err(|error| {
                        receipt_error(
                            AgentErrorCode::Io,
                            receipt_id,
                            Some(resource.clone()),
                            error.to_string(),
                        )
                    })?;
            let expected_backup_digest = recorded_backup_digest.ok_or_else(|| {
                receipt_error(
                    AgentErrorCode::Unsupported,
                    receipt_id,
                    Some(resource.clone()),
                    "Directory backup predates digest-protected rollback",
                )
            })?;
            if actual_backup_digest != expected_backup_digest {
                return Err(receipt_error(
                    AgentErrorCode::ResourceChanged,
                    receipt_id,
                    Some(resource.clone()),
                    "Directory backup changed after apply",
                ));
            }
            Ok(PlannedMutation {
                resource,
                kind: if current_kind == ResourceStateKind::Missing {
                    MutationKind::Create
                } else {
                    MutationKind::Replace
                },
                expected_digest,
                media_type: "application/vnd.ad.directory".into(),
                content: Some(serde_json::json!({
                    "path": backup_path,
                    "digest": actual_backup_digest,
                })),
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

fn contained_backup_name(
    receipt_id: &ReceiptId,
    operation_dir: &StateDirectory,
    backup_path: &Path,
) -> Result<String, AgentError> {
    let relative = backup_path
        .strip_prefix(operation_dir.display_path())
        .map_err(|_| {
            receipt_error(
                AgentErrorCode::PermissionDenied,
                receipt_id,
                None,
                "Backup path escapes its operation directory",
            )
        })?;
    let mut components = relative.components();
    let Some(std::path::Component::Normal(name)) = components.next() else {
        return Err(receipt_error(
            AgentErrorCode::InvalidPlan,
            receipt_id,
            None,
            "Backup path has no file name",
        ));
    };
    if components.next().is_some() {
        return Err(receipt_error(
            AgentErrorCode::PermissionDenied,
            receipt_id,
            None,
            "Backup path contains nested components",
        ));
    }
    name.to_str().map(str::to_owned).ok_or_else(|| {
        receipt_error(
            AgentErrorCode::InvalidPlan,
            receipt_id,
            None,
            "Backup file name is not UTF-8",
        )
    })
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
    intent: &PlanExecutionIntent,
    registry: &super::AdapterRegistry,
    faults: &dyn FaultInjector,
    state: &ExecutionState,
) -> Result<OperationReceipt, AgentError> {
    let receipt_id = ReceiptId::from(uuid::Uuid::new_v4().to_string());
    let mut journal = OperationJournalHandle::prepare(
        &plan,
        &receipt_id,
        execution_instance_id(),
        plan.id.as_str(),
        state,
    )
    .map_err(|error| io_error(&plan, None, error.to_string()))?;
    let operation_name = receipt_id.as_str().to_owned();
    let operation_dir = state
        .backups()
        .create_directory(&operation_name)
        .map_err(|error| io_error(&plan, None, error.to_string()))?;

    let preparation = (|| -> Result<(Vec<ResolvedMutation>, ContentDigest), AgentError> {
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
            let confined_target = ConfinedTarget::resolve_bound(
                &mutation.resource,
                target.path(),
                intent.project_root_identity,
            )?;
            let original = confined_target.observe()?;
            ensure_storage_compatible(&plan, &mutation.resource, target.storage(), &original)?;
            ensure_expected(&plan, &mutation, original.digest().as_ref())?;
            let (ownership_before, ownership_artifact) =
                prepare_ownership_authority(&plan, &mutation, &target, &original, intent, state)?;
            let directory_source =
                resolve_state_directory_source(&plan, &mutation, target.storage(), state)?;
            validate_mutation_source(
                &plan,
                &mutation,
                target.storage(),
                directory_source.as_ref(),
            )?;
            fail_if_requested(&plan, faults, ExecutionStep::Backup(index))?;
            let (backup_path, backup_digest) = match &original {
                TargetState::File(bytes) => {
                    let name = format!("{index}.backup");
                    operation_dir.write_atomic(&name, bytes).map_err(|error| {
                        io_error(&plan, Some(mutation.resource.clone()), error.to_string())
                    })?;
                    let path = operation_dir.display_path().join(&name);
                    (Some(path), Some(ContentDigest::sha256(bytes)))
                }
                TargetState::Directory(_) => {
                    let name = format!("{index}.backup");
                    confined_target.copy_directory_to(&operation_dir, &name)?;
                    let digest = operation_dir.directory_digest(&name).map_err(|error| {
                        io_error(&plan, Some(mutation.resource.clone()), error.to_string())
                    })?;
                    let path = operation_dir.display_path().join(&name);
                    (Some(path), Some(digest))
                }
                TargetState::Missing | TargetState::Symlink(_) => (None, None),
            };
            resolved.push(ResolvedMutation {
                mutation,
                target,
                confined_target,
                directory_source,
                original,
                backup_name: backup_path
                    .as_ref()
                    .and_then(|path| path.file_name())
                    .and_then(|name| name.to_str())
                    .map(str::to_owned),
                backup_path,
                backup_digest,
                ownership_before,
                ownership_artifact,
            });
        }

        let manifest = BackupManifest {
            plan_id: plan.id.clone(),
            entries: resolved.iter().map(manifest_entry).collect(),
        };
        let manifest_bytes = serde_json::to_vec_pretty(&manifest)
            .map_err(|error| io_error(&plan, None, error.to_string()))?;
        operation_dir
            .write_atomic("manifest.json", &manifest_bytes)
            .map_err(|error| io_error(&plan, None, error.to_string()))?;
        let manifest_digest = ContentDigest::sha256(&manifest_bytes);
        Ok((resolved, manifest_digest))
    })();
    let (resolved, manifest_digest) = match preparation {
        Ok(prepared) => prepared,
        Err(error) => {
            journal
                .transition(OperationJournalState::Compensated, None)
                .map_err(|journal_error| io_error(&plan, None, journal_error.to_string()))?;
            return Err(cleanup_pre_apply_failure(
                &plan,
                state,
                &operation_name,
                error,
            ));
        }
    };

    fail_if_requested(&plan, faults, ExecutionStep::PersistJournalApplying)
        .map_err(|error| cleanup_pre_apply_failure(&plan, state, &operation_name, error))?;
    journal
        .transition(OperationJournalState::Applying, None)
        .map_err(|error| {
            cleanup_pre_apply_failure(
                &plan,
                state,
                &operation_name,
                io_error(&plan, None, error.to_string()),
            )
        })?;

    let mut applied = Vec::new();
    for (index, item) in resolved.iter().enumerate() {
        let current = observe_resolved(item)?;
        if current.kind() != item.original.kind() || current.digest() != item.original.digest() {
            let error = changed_error(&plan, &item.mutation.resource);
            return finish_failed(
                &plan,
                receipt_id,
                error,
                manifest_digest,
                FailureContext {
                    resolved: &resolved,
                    applied: &applied,
                    state,
                    operation_dir: &operation_dir,
                    operation_name: &operation_name,
                    faults,
                    journal: &mut journal,
                    intent,
                },
            );
        }
        let pre_publish =
            fail_if_requested(&plan, faults, ExecutionStep::Apply(index)).and_then(|()| {
                validate_mutation_source(
                    &plan,
                    &item.mutation,
                    item.target.storage(),
                    item.directory_source.as_ref(),
                )
            });
        if let Err(error) = pre_publish {
            return finish_failed(
                &plan,
                receipt_id,
                error,
                manifest_digest,
                FailureContext {
                    resolved: &resolved,
                    applied: &applied,
                    state,
                    operation_dir: &operation_dir,
                    operation_name: &operation_name,
                    faults,
                    journal: &mut journal,
                    intent,
                },
            );
        }
        applied.push(index);
        let apply_result = apply_mutation(&plan, item)
            .and_then(|()| fail_if_requested(&plan, faults, ExecutionStep::ApplyPublished(index)));
        if let Err(error) = apply_result {
            return finish_failed(
                &plan,
                receipt_id,
                error,
                manifest_digest,
                FailureContext {
                    resolved: &resolved,
                    applied: &applied,
                    state,
                    operation_dir: &operation_dir,
                    operation_name: &operation_name,
                    faults,
                    journal: &mut journal,
                    intent,
                },
            );
        }
    }

    let receipt =
        fail_if_requested(&plan, faults, ExecutionStep::ConstructReceipt).and_then(|()| {
            receipt(
                &plan,
                receipt_id,
                ReceiptDraft {
                    status: OperationStatus::Complete,
                    resolved: &resolved,
                    applied: &applied,
                    manifest_digest: Some(manifest_digest),
                    message: None,
                    intent,
                },
            )
        });
    let receipt = match receipt {
        Ok(receipt) => receipt,
        Err(error) => {
            return compensate_after_receipt_failure(
                &plan,
                error,
                "constructed",
                FailureContext {
                    resolved: &resolved,
                    applied: &applied,
                    state,
                    operation_dir: &operation_dir,
                    operation_name: &operation_name,
                    faults,
                    journal: &mut journal,
                    intent,
                },
            )
        }
    };
    if let Err(mut error) = persist_receipt_with_faults(&plan, &receipt, faults, state) {
        match receipt_is_published(&plan, &receipt, state) {
            Ok(false) => {
                return compensate_after_receipt_failure(
                    &plan,
                    error,
                    "persisted",
                    FailureContext {
                        resolved: &resolved,
                        applied: &applied,
                        state,
                        operation_dir: &operation_dir,
                        operation_name: &operation_name,
                        faults,
                        journal: &mut journal,
                        intent,
                    },
                )
            }
            Ok(true) => {
                if let Err(sync_error) = state.history().sync() {
                    error.code = AgentErrorCode::PartialFailure;
                    error.retryable = false;
                    error.message = format!(
                        "Operation receipt was published but its directory could not be synced; recovery evidence was retained: {sync_error}"
                    );
                    return Err(error);
                }
            }
            Err(mut visibility_error) => {
                visibility_error.code = AgentErrorCode::PartialFailure;
                visibility_error.retryable = false;
                visibility_error.message = format!(
                    "Operation receipt publication is indeterminate; recovery evidence was retained: {}",
                    visibility_error.message
                );
                return Err(visibility_error);
            }
        }
    }
    if let Err(mut error) = apply_ownership_changes(state, &receipt.ownership_changes) {
        error.code = AgentErrorCode::PartialFailure;
        error.retryable = false;
        error.message = format!(
            "Operation receipt was persisted but ownership reconciliation failed: {}",
            error.message
        );
        journal
            .transition(OperationJournalState::RepairRequired, Some(&receipt.id))
            .map_err(|journal_error| io_error(&plan, None, journal_error.to_string()))?;
        return Err(error);
    }
    journal
        .transition(OperationJournalState::Committed, Some(&receipt.id))
        .map_err(|error| io_error(&plan, None, error.to_string()))?;
    Ok(receipt)
}

fn execution_state_error(error: std::io::Error) -> AgentError {
    AgentError {
        code: AgentErrorCode::PermissionDenied,
        message: format!("Failed to open the confined AD execution state: {error}"),
        agent_id: None,
        installation_id: None,
        resource: None,
        retryable: false,
        details: None,
    }
}

fn cleanup_pre_apply_failure(
    plan: &MutationPlan,
    state: &ExecutionState,
    operation_name: &str,
    mut cause: AgentError,
) -> AgentError {
    if let Err(error) = state.backups().remove(operation_name) {
        if error.kind() != std::io::ErrorKind::NotFound {
            cause.code = AgentErrorCode::Io;
            cause.retryable = true;
            cause.message = format!(
                "{}; failed to clean incomplete operation backups: {error}",
                cause.message
            );
            cause.agent_id = Some(plan.agent_id.clone());
            cause.installation_id = Some(plan.context.installation_id.clone());
        }
    }
    cause
}

fn finish_failed(
    plan: &MutationPlan,
    receipt_id: ReceiptId,
    cause: AgentError,
    manifest_digest: ContentDigest,
    context: FailureContext<'_>,
) -> Result<OperationReceipt, AgentError> {
    if context.applied.is_empty() {
        context
            .journal
            .transition(OperationJournalState::Compensated, None)
            .map_err(|error| io_error(plan, None, error.to_string()))?;
        return Err(cause);
    }
    let mut compensation_errors = Vec::new();
    for index in context.applied.iter().rev() {
        let restore_result =
            fail_if_requested(plan, context.faults, ExecutionStep::Compensate(*index)).and_then(
                |()| restore_target(plan, &context.resolved[*index], context.operation_dir),
            );
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
    let receipt = match receipt(
        plan,
        receipt_id,
        ReceiptDraft {
            status,
            resolved: context.resolved,
            applied: context.applied,
            manifest_digest: Some(manifest_digest),
            message: Some(message),
            intent: context.intent,
        },
    ) {
        Ok(receipt) => receipt,
        Err(error) => {
            return Err(unrecorded_result_error(
                plan,
                status,
                context.journal,
                error,
            ))
        }
    };
    if let Err(error) = persist_receipt_with_faults(plan, &receipt, context.faults, context.state) {
        return Err(unrecorded_result_error(
            plan,
            status,
            context.journal,
            error,
        ));
    }
    let journal_state = if status == OperationStatus::Compensated {
        OperationJournalState::Compensated
    } else {
        OperationJournalState::RepairRequired
    };
    context
        .journal
        .transition(journal_state, Some(&receipt.id))
        .map_err(|error| io_error(plan, None, error.to_string()))?;
    Ok(receipt)
}

fn unrecorded_result_error(
    plan: &MutationPlan,
    status: OperationStatus,
    journal: &mut OperationJournalHandle,
    mut error: AgentError,
) -> AgentError {
    let journal_state = if status == OperationStatus::Compensated {
        OperationJournalState::Compensated
    } else {
        error.code = AgentErrorCode::PartialFailure;
        error.retryable = false;
        OperationJournalState::RepairRequired
    };
    if let Err(journal_error) = journal.transition(journal_state, None) {
        error.message = format!(
            "{}; failed to persist {:?} journal state: {journal_error}",
            error.message, journal_state
        );
    }
    error.agent_id = Some(plan.agent_id.clone());
    error.installation_id = Some(plan.context.installation_id.clone());
    error
}

fn compensate_after_receipt_failure(
    plan: &MutationPlan,
    mut receipt_error: AgentError,
    failure_action: &str,
    context: FailureContext<'_>,
) -> Result<OperationReceipt, AgentError> {
    let mut compensation_errors = Vec::new();
    for index in context.applied.iter().rev() {
        let restore_result =
            fail_if_requested(plan, context.faults, ExecutionStep::Compensate(*index)).and_then(
                |()| restore_target(plan, &context.resolved[*index], context.operation_dir),
            );
        if let Err(error) = restore_result {
            compensation_errors.push(error.message);
        }
    }
    if compensation_errors.is_empty() {
        if let Err(error) = context
            .journal
            .transition(OperationJournalState::Compensated, None)
        {
            receipt_error.message = format!(
                "{}; failed to persist compensated journal state: {error}",
                receipt_error.message
            );
        }
        receipt_error.message = format!(
            "Operation receipt could not be {failure_action}; applied changes were compensated: {}",
            receipt_error.message
        );
        receipt_error =
            cleanup_pre_apply_failure(plan, context.state, context.operation_name, receipt_error);
    } else {
        if let Err(error) = context
            .journal
            .transition(OperationJournalState::RepairRequired, None)
        {
            compensation_errors.push(format!(
                "failed to persist repair-required journal: {error}"
            ));
        }
        receipt_error.code = AgentErrorCode::PartialFailure;
        receipt_error.retryable = false;
        receipt_error.message = format!(
            "Operation receipt could not be {failure_action} and compensation failed: {}; {}",
            receipt_error.message,
            compensation_errors.join("; ")
        );
    }
    Err(receipt_error)
}

fn fail_if_requested(
    plan: &MutationPlan,
    faults: &dyn FaultInjector,
    step: ExecutionStep,
) -> Result<(), AgentError> {
    faults.before_step(step);
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
        )
        || matches!(
            (storage, state),
            (ResourceStorage::Directory, TargetState::Directory(_))
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

fn prepare_ownership_authority(
    plan: &MutationPlan,
    mutation: &PlannedMutation,
    target: &ManagedResourceTarget,
    original: &TargetState,
    intent: &PlanExecutionIntent,
    state: &ExecutionState,
) -> Result<(Option<ResourceOwnershipRecord>, Option<OwnershipArtifact>), AgentError> {
    if !ownership_managed(&mutation.resource, target.storage()) {
        return Ok((None, None));
    }
    let current = load_ownership_record(state, &mutation.resource)?;
    let target_id = PhysicalTargetId::for_resource(&mutation.resource);
    if intent.operation_kind == OperationKind::Rollback {
        let restore = intent.ownership_restores.get(&target_id).ok_or_else(|| {
            ownership_plan_error(
                plan,
                &mutation.resource,
                "Rollback plan is missing ownership evidence",
            )
        })?;
        if current != restore.expected_current {
            return Err(ownership_plan_error(
                plan,
                &mutation.resource,
                "Ownership changed after rollback preview",
            ));
        }
        if let Some(record) = current.as_ref() {
            if restore.expected_matches_target {
                validate_ownership_record(
                    record,
                    &mutation.resource,
                    target.path(),
                    original.kind(),
                    original.digest().as_ref(),
                )?;
            }
            validate_ownership_artifact(record)?;
        }
        return Ok((current, None));
    }

    match original {
        TargetState::Missing => {
            if current.is_some() || mutation.kind != MutationKind::Create {
                return Err(ownership_plan_error(
                    plan,
                    &mutation.resource,
                    "Missing target has stale or incompatible ownership",
                ));
            }
        }
        _ => {
            let record = current.as_ref().ok_or_else(|| {
                ownership_plan_error(
                    plan,
                    &mutation.resource,
                    "Project resource is not owned by AD",
                )
            })?;
            if mutation.kind == MutationKind::Replace
                && target.storage() == ResourceStorage::Directory
            {
                validate_ownership_record_identity(
                    record,
                    &mutation.resource,
                    target.path(),
                    original.kind(),
                )?;
            } else {
                validate_ownership_record(
                    record,
                    &mutation.resource,
                    target.path(),
                    original.kind(),
                    original.digest().as_ref(),
                )?;
                validate_ownership_artifact(record)?;
            }
        }
    }

    let artifact = match mutation.kind {
        MutationKind::Delete => None,
        MutationKind::Create | MutationKind::Replace => Some(mutation_ownership_artifact(
            plan,
            mutation,
            target.storage(),
        )?),
    };
    Ok((current, artifact))
}

fn mutation_ownership_artifact(
    plan: &MutationPlan,
    mutation: &PlannedMutation,
    storage: ResourceStorage,
) -> Result<OwnershipArtifact, AgentError> {
    match storage {
        ResourceStorage::Symlink => match parse_symlink_source(plan, mutation)? {
            SymlinkMutationSource::Checked { path, digest } => {
                Ok(OwnershipArtifact { id: path, digest })
            }
            SymlinkMutationSource::Legacy(_) => Err(ownership_plan_error(
                plan,
                &mutation.resource,
                "Ownership-managed symlinks require a checked artifact digest",
            )),
        },
        ResourceStorage::Directory => {
            let source = parse_directory_source(plan, mutation)?;
            Ok(OwnershipArtifact {
                id: source.path,
                digest: source.digest,
            })
        }
        ResourceStorage::File => Err(ownership_plan_error(
            plan,
            &mutation.resource,
            "File resources do not use collection ownership records",
        )),
    }
}

fn ownership_plan_error(
    plan: &MutationPlan,
    resource: &ResourceRef,
    message: impl Into<String>,
) -> AgentError {
    AgentError {
        code: AgentErrorCode::PermissionDenied,
        message: message.into(),
        agent_id: Some(plan.agent_id.clone()),
        installation_id: Some(resource.installation_id.clone()),
        resource: Some(resource.clone()),
        retryable: false,
        details: Some(serde_json::json!({"phase": "resource_ownership"})),
    }
}

fn validate_mutation_source(
    plan: &MutationPlan,
    mutation: &PlannedMutation,
    storage: ResourceStorage,
    directory_source: Option<&StateDirectory>,
) -> Result<(), AgentError> {
    if mutation.kind == MutationKind::Delete {
        return Ok(());
    }
    match storage {
        ResourceStorage::Directory => {
            let source = parse_directory_source(plan, mutation)?;
            let actual = match directory_source {
                Some(source) => source
                    .digest()
                    .map_err(|_| changed_error(plan, &mutation.resource))?,
                None => directory_tree_digest(Path::new(&source.path))
                    .map_err(|_| changed_error(plan, &mutation.resource))?,
            };
            if actual != source.digest {
                return Err(changed_error(plan, &mutation.resource));
            }
        }
        ResourceStorage::Symlink => {
            let source = parse_symlink_source(plan, mutation)?;
            if let Some(expected) = source.digest() {
                let path = Path::new(source.path());
                let metadata = std::fs::symlink_metadata(path)
                    .map_err(|_| changed_error(plan, &mutation.resource))?;
                let actual = if metadata.file_type().is_symlink() {
                    return Err(changed_error(plan, &mutation.resource));
                } else if metadata.is_dir() {
                    directory_tree_digest(path)
                        .map_err(|_| changed_error(plan, &mutation.resource))?
                } else if metadata.is_file() {
                    let bytes =
                        std::fs::read(path).map_err(|_| changed_error(plan, &mutation.resource))?;
                    ContentDigest::sha256(&bytes)
                } else {
                    return Err(changed_error(plan, &mutation.resource));
                };
                if &actual != expected {
                    return Err(changed_error(plan, &mutation.resource));
                }
            }
        }
        ResourceStorage::File => {}
    }
    Ok(())
}

fn resolve_state_directory_source(
    plan: &MutationPlan,
    mutation: &PlannedMutation,
    storage: ResourceStorage,
    state: &ExecutionState,
) -> Result<Option<StateDirectory>, AgentError> {
    if storage != ResourceStorage::Directory || mutation.kind == MutationKind::Delete {
        return Ok(None);
    }
    let source = parse_directory_source(plan, mutation)?;
    let path = Path::new(&source.path);
    let Ok(relative) = path.strip_prefix(state.backups().display_path()) else {
        return Ok(None);
    };
    let mut components = relative.components();
    let Some(std::path::Component::Normal(operation)) = components.next() else {
        return Err(changed_error(plan, &mutation.resource));
    };
    let Some(std::path::Component::Normal(backup)) = components.next() else {
        return Err(changed_error(plan, &mutation.resource));
    };
    if components.next().is_some() {
        return Err(changed_error(plan, &mutation.resource));
    }
    let operation = operation
        .to_str()
        .ok_or_else(|| changed_error(plan, &mutation.resource))?;
    let backup = backup
        .to_str()
        .ok_or_else(|| changed_error(plan, &mutation.resource))?;
    state
        .backups()
        .open_directory(operation)
        .and_then(|directory| directory.open_directory(backup))
        .map(Some)
        .map_err(|_| changed_error(plan, &mutation.resource))
}

fn parse_symlink_source(
    plan: &MutationPlan,
    mutation: &PlannedMutation,
) -> Result<SymlinkMutationSource, AgentError> {
    let content = mutation.content.clone().ok_or_else(|| {
        plan_error(
            plan,
            Some(mutation.resource.clone()),
            "Symlink mutation requires a target reference",
        )
    })?;
    serde_json::from_value(content).map_err(|error| {
        plan_error(
            plan,
            Some(mutation.resource.clone()),
            format!("Invalid symlink target reference: {error}"),
        )
    })
}

fn parse_directory_source(
    plan: &MutationPlan,
    mutation: &PlannedMutation,
) -> Result<DirectoryMutationSource, AgentError> {
    let content = mutation.content.clone().ok_or_else(|| {
        plan_error(
            plan,
            Some(mutation.resource.clone()),
            "Directory mutation requires a source reference",
        )
    })?;
    serde_json::from_value(content).map_err(|error| {
        plan_error(
            plan,
            Some(mutation.resource.clone()),
            format!("Invalid directory source reference: {error}"),
        )
    })
}

fn apply_mutation(plan: &MutationPlan, item: &ResolvedMutation) -> Result<(), AgentError> {
    match item.mutation.kind {
        MutationKind::Delete => item.confined_target.remove(),
        MutationKind::Create | MutationKind::Replace => match item.target.storage() {
            ResourceStorage::File => {
                let bytes = render_content(&item.mutation)?;
                item.confined_target.write_atomic(&bytes)
            }
            ResourceStorage::Symlink => {
                let source = parse_symlink_source(plan, &item.mutation)?;
                item.confined_target
                    .write_symlink_atomic(Path::new(source.path()))
            }
            ResourceStorage::Directory => {
                if let Some(source) = &item.directory_source {
                    item.confined_target.write_directory_from(source)
                } else {
                    let source = parse_directory_source(plan, &item.mutation)?;
                    item.confined_target
                        .write_directory_atomic(Path::new(&source.path))
                }
            }
        },
    }
}

fn restore_target(
    plan: &MutationPlan,
    item: &ResolvedMutation,
    operation_dir: &StateDirectory,
) -> Result<(), AgentError> {
    match &item.original {
        TargetState::Missing => item.confined_target.remove(),
        TargetState::File(_) => {
            let backup_name = item.backup_name.as_deref().ok_or_else(|| {
                io_error(
                    plan,
                    Some(item.mutation.resource.clone()),
                    "Missing file backup name",
                )
            })?;
            let bytes = operation_dir.read(backup_name).map_err(|error| {
                io_error(
                    plan,
                    Some(item.mutation.resource.clone()),
                    error.to_string(),
                )
            })?;
            item.confined_target.write_atomic(&bytes)
        }
        TargetState::Symlink(source) => item.confined_target.write_symlink_atomic(source),
        TargetState::Directory(_) => {
            let backup_name = item.backup_name.as_deref().ok_or_else(|| {
                io_error(
                    plan,
                    Some(item.mutation.resource.clone()),
                    "Missing directory backup name",
                )
            })?;
            let backup = operation_dir.open_directory(backup_name).map_err(|error| {
                io_error(
                    plan,
                    Some(item.mutation.resource.clone()),
                    error.to_string(),
                )
            })?;
            item.confined_target.write_directory_from(&backup)
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
        TargetState::Directory(_) => ("directory".into(), None),
    };
    BackupManifestEntry {
        resource: item.mutation.resource.clone(),
        target_path: item.target.path().to_string_lossy().into_owned(),
        original_kind,
        backup_path: item
            .backup_path
            .as_ref()
            .map(|path| path.to_string_lossy().into_owned()),
        backup_digest: item.backup_digest.clone(),
        link_target,
        ownership_before: item.ownership_before.clone(),
    }
}

fn observe_resolved(item: &ResolvedMutation) -> Result<TargetState, AgentError> {
    item.confined_target.observe()
}

fn receipt(
    plan: &MutationPlan,
    id: ReceiptId,
    draft: ReceiptDraft<'_>,
) -> Result<OperationReceipt, AgentError> {
    let post_apply_states = draft
        .applied
        .iter()
        .map(|index| {
            let item = &draft.resolved[*index];
            let state = observe_resolved(item)?;
            Ok(AppliedResourceState {
                resource: item.mutation.resource.clone(),
                kind: state.kind(),
                digest: state.digest(),
            })
        })
        .collect::<Result<Vec<_>, AgentError>>()?;
    let ownership_changes = if draft.status == OperationStatus::Complete {
        draft
            .applied
            .iter()
            .zip(post_apply_states.iter())
            .filter_map(|(index, state)| {
                ownership_change(&draft.resolved[*index], state, &id, draft.intent).transpose()
            })
            .collect::<Result<Vec<_>, AgentError>>()?
    } else {
        Vec::new()
    };
    Ok(OperationReceipt {
        schema_version: OPERATION_RECEIPT_SCHEMA_VERSION,
        id,
        plan_id: plan.id.clone(),
        operation_kind: draft.intent.operation_kind,
        parent_receipt_id: draft.intent.parent_receipt_id.clone(),
        context: Some(plan.context.clone()),
        workspace_key: workspace_key_for_context(&plan.context),
        action_id: (draft.intent.operation_kind == OperationKind::Rollback)
            .then(|| "rollback".to_owned()),
        status: draft.status,
        applied_resources: draft
            .applied
            .iter()
            .map(|index| draft.resolved[*index].mutation.resource.clone())
            .collect(),
        backup_paths: draft
            .resolved
            .iter()
            .filter_map(|item| item.backup_path.as_ref())
            .map(|path| path.to_string_lossy().into_owned())
            .collect(),
        post_apply_states,
        manifest_digest: draft.manifest_digest,
        ownership_changes,
        ownership_evidence_version: OWNERSHIP_EVIDENCE_VERSION,
        rollback: if draft.intent.operation_kind == OperationKind::Rollback {
            RollbackEligibility::unavailable(RollbackUnavailableReason::RollbackReceipt)
        } else if draft.status == OperationStatus::Compensated || draft.applied.is_empty() {
            RollbackEligibility::unavailable(RollbackUnavailableReason::Compensated)
        } else {
            RollbackEligibility::available()
        },
        created_at: Some(Utc::now()),
        message: draft.message,
    })
}

fn ownership_change(
    item: &ResolvedMutation,
    state: &AppliedResourceState,
    receipt_id: &ReceiptId,
    intent: &PlanExecutionIntent,
) -> Result<Option<ResourceOwnershipChange>, AgentError> {
    if !ownership_managed(&item.mutation.resource, item.target.storage()) {
        return Ok(None);
    }
    let record_id = ownership_record_id(&item.mutation.resource);
    if intent.operation_kind == OperationKind::Rollback {
        let restore = intent
            .ownership_restores
            .get(&PhysicalTargetId::for_resource(&item.mutation.resource))
            .ok_or_else(|| AgentError {
                code: AgentErrorCode::InvalidPlan,
                message: "Rollback ownership evidence disappeared during execution".into(),
                agent_id: None,
                installation_id: Some(item.mutation.resource.installation_id.clone()),
                resource: Some(item.mutation.resource.clone()),
                retryable: false,
                details: Some(serde_json::json!({"phase": "resource_ownership"})),
            })?;
        return match restore.restore.as_ref() {
            Some(record) => {
                if state.kind != record.target_kind
                    || state.digest.as_ref() != Some(&record.target_digest)
                {
                    return Err(AgentError {
                        code: AgentErrorCode::PartialFailure,
                        message: "Restored target does not match its ownership record".into(),
                        agent_id: None,
                        installation_id: Some(item.mutation.resource.installation_id.clone()),
                        resource: Some(item.mutation.resource.clone()),
                        retryable: false,
                        details: Some(serde_json::json!({"phase": "resource_ownership"})),
                    });
                }
                let mut record = record.clone();
                record.updated_by_receipt_id = receipt_id.clone();
                Ok(Some(ResourceOwnershipChange {
                    kind: ResourceOwnershipChangeKind::Upsert,
                    record_id,
                    previous_record: restore.expected_current.clone(),
                    record: Some(record),
                }))
            }
            None if restore.expected_current.is_none() => Ok(None),
            None => Ok(Some(ResourceOwnershipChange {
                kind: ResourceOwnershipChangeKind::Remove,
                record_id,
                previous_record: restore.expected_current.clone(),
                record: None,
            })),
        };
    }

    match item.mutation.kind {
        MutationKind::Delete => Ok(Some(ResourceOwnershipChange {
            kind: ResourceOwnershipChangeKind::Remove,
            record_id,
            previous_record: item.ownership_before.clone(),
            record: None,
        })),
        MutationKind::Create | MutationKind::Replace => {
            let artifact = item.ownership_artifact.as_ref().ok_or_else(|| AgentError {
                code: AgentErrorCode::InvalidPlan,
                message: "Ownership-managed mutation is missing artifact evidence".into(),
                agent_id: None,
                installation_id: Some(item.mutation.resource.installation_id.clone()),
                resource: Some(item.mutation.resource.clone()),
                retryable: false,
                details: Some(serde_json::json!({"phase": "resource_ownership"})),
            })?;
            let target_digest = state.digest.clone().ok_or_else(|| AgentError {
                code: AgentErrorCode::PartialFailure,
                message: "Owned target has no post-apply digest".into(),
                agent_id: None,
                installation_id: Some(item.mutation.resource.installation_id.clone()),
                resource: Some(item.mutation.resource.clone()),
                retryable: false,
                details: Some(serde_json::json!({"phase": "resource_ownership"})),
            })?;
            let record = ResourceOwnershipRecord {
                schema_version: RESOURCE_OWNERSHIP_SCHEMA_VERSION,
                id: record_id.clone(),
                workspace_key: ownership_workspace_key(&item.mutation.resource)?,
                resource: item.mutation.resource.clone(),
                target_id: PhysicalTargetId::for_resource(&item.mutation.resource),
                target_path: item.target.path().to_string_lossy().into_owned(),
                target_kind: state.kind,
                target_digest,
                artifact_id: artifact.id.clone(),
                artifact_digest: artifact.digest.clone(),
                creating_receipt_id: item
                    .ownership_before
                    .as_ref()
                    .map(|record| record.creating_receipt_id.clone())
                    .unwrap_or_else(|| receipt_id.clone()),
                updated_by_receipt_id: receipt_id.clone(),
            };
            Ok(Some(ResourceOwnershipChange {
                kind: ResourceOwnershipChangeKind::Upsert,
                record_id,
                previous_record: item.ownership_before.clone(),
                record: Some(record),
            }))
        }
    }
}

fn persist_receipt(
    plan: &MutationPlan,
    receipt: &OperationReceipt,
    state: &ExecutionState,
) -> Result<(), AgentError> {
    let bytes = receipt_bytes(plan, receipt)?;
    state
        .history()
        .write_atomic_new(&format!("{}.json", receipt.id), &bytes)
        .map_err(|error| io_error(plan, None, error.to_string()))
}

fn receipt_is_published(
    plan: &MutationPlan,
    receipt: &OperationReceipt,
    state: &ExecutionState,
) -> Result<bool, AgentError> {
    let expected = receipt_bytes(plan, receipt)?;
    match state.history().read(&format!("{}.json", receipt.id)) {
        Ok(actual) if actual == expected => Ok(true),
        Ok(_) => Err(AgentError {
            code: AgentErrorCode::PermissionDenied,
            message: "Published operation receipt bytes do not match the execution result".into(),
            agent_id: Some(plan.agent_id.clone()),
            installation_id: Some(plan.context.installation_id.clone()),
            resource: None,
            retryable: false,
            details: Some(serde_json::json!({"phase": "operation_receipt"})),
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(io_error(plan, None, error.to_string())),
    }
}

fn receipt_bytes(plan: &MutationPlan, receipt: &OperationReceipt) -> Result<Vec<u8>, AgentError> {
    serde_json::to_vec_pretty(receipt).map_err(|error| io_error(plan, None, error.to_string()))
}

fn persist_receipt_with_faults(
    plan: &MutationPlan,
    receipt: &OperationReceipt,
    faults: &dyn FaultInjector,
    state: &ExecutionState,
) -> Result<(), AgentError> {
    fail_if_requested(plan, faults, ExecutionStep::PersistReceipt)?;
    persist_receipt(plan, receipt, state)?;
    fail_if_requested(plan, faults, ExecutionStep::PersistReceiptPublished)
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
