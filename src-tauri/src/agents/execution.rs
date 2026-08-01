use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};

use crate::fs::atomic::write_atomic;
use crate::fs::paths::{backups_dir, ensure_dir, history_dir};

use super::execution_confinement::{validate_ad_managed_root, ConfinedFileTarget};
use super::execution_fs::{
    copy_directory_tree, directory_tree_digest, observe_target, remove_target, render_content,
    write_directory_atomic, write_symlink_atomic, TargetState,
};
use super::{
    builtin_registry, execution_instance_id, AgentContext, AgentError, AgentErrorCode,
    AppliedResourceState, ContentDigest, ManagedResourceTarget, MutationKind, MutationPlan,
    OperationJournalHandle, OperationJournalState, OperationReceipt, OperationStatus,
    PlanAcknowledgement, PlanId, PlanStore, PlannedMutation, ReceiptId, ResourceKind, ResourceRef,
    ResourceScope, ResourceStateKind, ResourceStorage, TargetLockSet, WritePolicy,
};

static EXECUTION_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Default)]
pub struct ExecutionEngine;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum ExecutionStep {
    Backup(usize),
    Apply(usize),
    Compensate(usize),
    ConstructReceipt,
    PersistReceipt,
    PersistJournalApplying,
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
    confined_file: Option<ConfinedFileTarget>,
    original: TargetState,
    backup_path: Option<PathBuf>,
    backup_digest: Option<ContentDigest>,
}

struct FailureContext<'a> {
    resolved: &'a [ResolvedMutation],
    applied: &'a [usize],
    operation_dir: &'a Path,
    faults: &'a dyn FaultInjector,
    journal: &'a mut OperationJournalHandle,
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
        self.apply_internal(plan_id, plans, &NoFaults)
    }

    pub fn apply_confirmed(
        &self,
        plan_id: &PlanId,
        plans: &PlanStore,
    ) -> Result<OperationReceipt, AgentError> {
        self.apply_internal_with_confirmation(plan_id, plans, &NoFaults, true)
    }

    pub fn apply_acknowledged(
        &self,
        plan_id: &PlanId,
        plans: &PlanStore,
        acknowledgements: &[PlanAcknowledgement],
    ) -> Result<OperationReceipt, AgentError> {
        self.apply_internal_with_acknowledgements(plan_id, plans, &NoFaults, acknowledgements)
    }

    pub fn rollback(&self, receipt_id: &ReceiptId) -> Result<OperationReceipt, AgentError> {
        let _guard = EXECUTION_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        validate_ad_managed_root()?;
        let registry = builtin_registry();
        let plan = rollback_plan(receipt_id, &registry)?;
        let _target_locks = TargetLockSet::acquire_for_plan(&plan, &registry)?;
        let receipt = execute_plan(plan, &registry, &NoFaults)?;
        refresh_runtime_state(&receipt, false);
        Ok(receipt)
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
        let acknowledgements = confirmed.then_some(PlanAcknowledgement {
            code: super::PlanAcknowledgementCode::ConversionApply,
            accepted: true,
        });
        self.apply_internal_with_acknowledgements(
            plan_id,
            plans,
            faults,
            acknowledgements.as_slice(),
        )
    }

    fn apply_internal_with_acknowledgements(
        &self,
        plan_id: &PlanId,
        plans: &PlanStore,
        faults: &dyn FaultInjector,
        acknowledgements: &[PlanAcknowledgement],
    ) -> Result<OperationReceipt, AgentError> {
        let _guard = EXECUTION_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        validate_ad_managed_root()?;
        let registry = builtin_registry();
        let resources = plans.resources_for_locking(plan_id)?;
        let _target_locks =
            TargetLockSet::acquire_for_resources(&resources, plan_id.as_str(), &registry)?;
        let observe = |resource: &ResourceRef| {
            let context = AgentContext {
                installation_id: resource.installation_id.clone(),
                project_path: resource.project_path.clone(),
            };
            let target = registry.resolve_resource(&context, resource)?;
            Ok(observe_target(&target)?.digest())
        };
        let plan = plans.claim_acknowledged(plan_id, acknowledgements, observe)?;
        let refresh_base_config = plan_refreshes_base_config(&plan);
        let receipt = execute_plan(plan, &registry, faults)?;
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
        agent_id,
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
    let recorded_backup_digest = entry.backup_digest.clone();
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
        "directory" => {
            let backup_path = entry.backup_path.ok_or_else(|| {
                receipt_error(
                    AgentErrorCode::InvalidPlan,
                    receipt_id,
                    Some(resource.clone()),
                    "Directory rollback entry has no backup",
                )
            })?;
            let backup = contained_backup_path(receipt_id, operation_dir, Path::new(&backup_path))?;
            if !backup.is_dir() {
                return Err(receipt_error(
                    AgentErrorCode::InvalidPlan,
                    receipt_id,
                    Some(resource.clone()),
                    "Directory rollback backup is not a directory",
                ));
            }
            let actual_backup_digest = super::execution_fs::directory_tree_digest(&backup)
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
                    "path": backup.to_string_lossy(),
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

fn read_contained_backup(
    receipt_id: &ReceiptId,
    operation_dir: &Path,
    backup_path: &Path,
) -> Result<Vec<u8>, AgentError> {
    let canonical_backup = contained_backup_path(receipt_id, operation_dir, backup_path)?;
    std::fs::read(&canonical_backup)
        .map_err(|error| receipt_error(AgentErrorCode::Io, receipt_id, None, error.to_string()))
}

fn contained_backup_path(
    receipt_id: &ReceiptId,
    operation_dir: &Path,
    backup_path: &Path,
) -> Result<PathBuf, AgentError> {
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
    Ok(canonical_backup)
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
    let mut journal = OperationJournalHandle::prepare(
        &plan,
        &receipt_id,
        execution_instance_id(),
        plan.id.as_str(),
    )
    .map_err(|error| io_error(&plan, None, error.to_string()))?;
    let operation_dir = backups_dir()
        .map_err(|error| io_error(&plan, None, error.to_string()))?
        .join("operations")
        .join(receipt_id.as_str());
    ensure_dir(&operation_dir).map_err(|error| io_error(&plan, None, error.to_string()))?;

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
            let confined_file = if target.storage() == ResourceStorage::File {
                Some(ConfinedFileTarget::resolve(
                    &mutation.resource,
                    target.path(),
                )?)
            } else {
                None
            };
            let original = match &confined_file {
                Some(confined) => confined.observe()?,
                None => observe_target(&target)?,
            };
            ensure_storage_compatible(&plan, &mutation.resource, target.storage(), &original)?;
            ensure_expected(&plan, &mutation, original.digest().as_ref())?;
            validate_mutation_source(&plan, &mutation, target.storage())?;
            fail_if_requested(&plan, faults, ExecutionStep::Backup(index))?;
            let (backup_path, backup_digest) = match &original {
                TargetState::File(bytes) => {
                    let path = operation_dir.join(format!("{index}.backup"));
                    write_atomic(&path, bytes).map_err(|error| {
                        io_error(&plan, Some(mutation.resource.clone()), error.to_string())
                    })?;
                    (Some(path), Some(ContentDigest::sha256(bytes)))
                }
                TargetState::Directory(_) => {
                    let path = operation_dir.join(format!("{index}.backup"));
                    copy_directory_tree(target.path(), &path).map_err(|error| {
                        io_error(&plan, Some(mutation.resource.clone()), error.to_string())
                    })?;
                    let digest = directory_tree_digest(&path).map_err(|error| {
                        io_error(&plan, Some(mutation.resource.clone()), error.to_string())
                    })?;
                    (Some(path), Some(digest))
                }
                TargetState::Missing | TargetState::Symlink(_) => (None, None),
            };
            resolved.push(ResolvedMutation {
                mutation,
                target,
                confined_file,
                original,
                backup_path,
                backup_digest,
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
        Ok((resolved, manifest_digest))
    })();
    let (resolved, manifest_digest) = match preparation {
        Ok(prepared) => prepared,
        Err(error) => {
            journal
                .transition(OperationJournalState::Compensated, None)
                .map_err(|journal_error| io_error(&plan, None, journal_error.to_string()))?;
            return Err(cleanup_pre_apply_failure(&plan, &operation_dir, error));
        }
    };

    fail_if_requested(&plan, faults, ExecutionStep::PersistJournalApplying)
        .map_err(|error| cleanup_pre_apply_failure(&plan, &operation_dir, error))?;
    journal
        .transition(OperationJournalState::Applying, None)
        .map_err(|error| {
            cleanup_pre_apply_failure(
                &plan,
                &operation_dir,
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
                    operation_dir: &operation_dir,
                    faults,
                    journal: &mut journal,
                },
            );
        }
        let apply_result = fail_if_requested(&plan, faults, ExecutionStep::Apply(index))
            .and_then(|()| validate_mutation_source(&plan, &item.mutation, item.target.storage()))
            .and_then(|()| apply_mutation(&plan, item));
        if let Err(error) = apply_result {
            return finish_failed(
                &plan,
                receipt_id,
                error,
                manifest_digest,
                FailureContext {
                    resolved: &resolved,
                    applied: &applied,
                    operation_dir: &operation_dir,
                    faults,
                    journal: &mut journal,
                },
            );
        }
        applied.push(index);
    }

    let receipt =
        fail_if_requested(&plan, faults, ExecutionStep::ConstructReceipt).and_then(|()| {
            receipt(
                &plan,
                receipt_id,
                OperationStatus::Complete,
                &resolved,
                &applied,
                Some(manifest_digest),
                None,
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
                    operation_dir: &operation_dir,
                    faults,
                    journal: &mut journal,
                },
            )
        }
    };
    if let Err(error) = persist_receipt_with_faults(&plan, &receipt, faults) {
        return compensate_after_receipt_failure(
            &plan,
            error,
            "persisted",
            FailureContext {
                resolved: &resolved,
                applied: &applied,
                operation_dir: &operation_dir,
                faults,
                journal: &mut journal,
            },
        );
    }
    journal
        .transition(OperationJournalState::Committed, Some(&receipt.id))
        .map_err(|error| io_error(&plan, None, error.to_string()))?;
    Ok(receipt)
}

fn cleanup_pre_apply_failure(
    plan: &MutationPlan,
    operation_dir: &Path,
    mut cause: AgentError,
) -> AgentError {
    if let Err(error) = std::fs::remove_dir_all(operation_dir) {
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
            fail_if_requested(plan, context.faults, ExecutionStep::Compensate(*index))
                .and_then(|()| restore_target(plan, &context.resolved[*index]));
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
        status,
        context.resolved,
        context.applied,
        Some(manifest_digest),
        Some(message),
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
    if let Err(error) = persist_receipt_with_faults(plan, &receipt, context.faults) {
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
            fail_if_requested(plan, context.faults, ExecutionStep::Compensate(*index))
                .and_then(|()| restore_target(plan, &context.resolved[*index]));
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
        receipt_error = cleanup_pre_apply_failure(plan, context.operation_dir, receipt_error);
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

fn validate_mutation_source(
    plan: &MutationPlan,
    mutation: &PlannedMutation,
    storage: ResourceStorage,
) -> Result<(), AgentError> {
    if mutation.kind == MutationKind::Delete {
        return Ok(());
    }
    match storage {
        ResourceStorage::Directory => {
            let source = parse_directory_source(plan, mutation)?;
            let actual = directory_tree_digest(Path::new(&source.path))
                .map_err(|_| changed_error(plan, &mutation.resource))?;
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
        MutationKind::Delete if item.target.storage() == ResourceStorage::File => item
            .confined_file
            .as_ref()
            .expect("file targets are confined during resolution")
            .remove(),
        MutationKind::Delete => remove_target(item.target.path()),
        MutationKind::Create | MutationKind::Replace => match item.target.storage() {
            ResourceStorage::File => {
                let bytes = render_content(&item.mutation)?;
                item.confined_file
                    .as_ref()
                    .expect("file targets are confined during resolution")
                    .write_atomic(&bytes)
            }
            ResourceStorage::Symlink => {
                let source = parse_symlink_source(plan, &item.mutation)?;
                write_symlink_atomic(item.target.path(), Path::new(source.path())).map_err(
                    |error| {
                        io_error(
                            plan,
                            Some(item.mutation.resource.clone()),
                            error.to_string(),
                        )
                    },
                )
            }
            ResourceStorage::Directory => {
                let source = parse_directory_source(plan, &item.mutation)?;
                write_directory_atomic(item.target.path(), Path::new(&source.path)).map_err(
                    |error| {
                        io_error(
                            plan,
                            Some(item.mutation.resource.clone()),
                            error.to_string(),
                        )
                    },
                )
            }
        },
    }
}

fn restore_target(plan: &MutationPlan, item: &ResolvedMutation) -> Result<(), AgentError> {
    match &item.original {
        TargetState::Missing if item.target.storage() == ResourceStorage::File => item
            .confined_file
            .as_ref()
            .expect("file targets are confined during resolution")
            .remove(),
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
            item.confined_file
                .as_ref()
                .expect("file targets are confined during resolution")
                .write_atomic(&bytes)
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
        TargetState::Directory(_) => {
            let backup = item.backup_path.as_ref().ok_or_else(|| {
                io_error(
                    plan,
                    Some(item.mutation.resource.clone()),
                    "Missing directory backup",
                )
            })?;
            write_directory_atomic(item.target.path(), backup).map_err(|error| {
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
    }
}

fn observe_resolved(item: &ResolvedMutation) -> Result<TargetState, AgentError> {
    match &item.confined_file {
        Some(confined) => confined.observe(),
        None => observe_target(&item.target),
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
            let state = observe_resolved(item)?;
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
        .map_err(|error| io_error(plan, None, error.to_string()))?;
    std::fs::File::open(&directory)
        .and_then(|file| file.sync_all())
        .map_err(|error| io_error(plan, None, error.to_string()))
}

fn persist_receipt_with_faults(
    plan: &MutationPlan,
    receipt: &OperationReceipt,
    faults: &dyn FaultInjector,
) -> Result<(), AgentError> {
    fail_if_requested(plan, faults, ExecutionStep::PersistReceipt)?;
    persist_receipt(plan, receipt)
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
