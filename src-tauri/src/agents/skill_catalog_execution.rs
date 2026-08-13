use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::execution_lock::{execution_instance_id, TargetLockSet};
use super::execution_state::ExecutionState;
use super::resource_catalog::{
    persist_resource_catalog_projection, resource_catalog_lock_target,
    resource_lifecycle_lock_target,
};
use super::skill_catalog::{load_skill_catalog_state_from, SkillCatalogError};
use super::skill_catalog_plans::{
    ClaimedSkillCatalogPlan, SkillCatalogAction, SkillCatalogPlanError,
};
use super::{
    catalog_resource_id, cleanup_unpublished_skill_staging, observe_skill_source_revision,
    publish_staged_git_skill_source_binding, reconcile_git_skill_source_current,
    skill_catalog_plan_references_are_current, switch_git_skill_source_binding, ContentDigest,
    PlanId, ReceiptId, ResourceKind, ResourceRef, SkillArtifactError, SkillArtifactRef,
    SkillCatalogEntry, SkillCatalogPlanClaim, SkillCatalogPlanStore, SkillCatalogPlanView,
    SkillSourceBinding, SkillSourceType, WorkspaceKey,
};
use crate::fs::paths::skill_catalog_path;

const JOURNAL_SCHEMA_VERSION: u32 = 1;
const RECEIPT_SCHEMA_VERSION: u32 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillCatalogOperationOutcome {
    Changed,
    NoChange,
    Compensated,
    PartialFailure,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillCatalogReceiptStatus {
    Complete,
    Compensated,
    Recovered,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SkillCatalogReceipt {
    pub schema_version: u32,
    pub id: ReceiptId,
    pub plan_id: PlanId,
    pub action: SkillCatalogAction,
    pub source_id: String,
    pub before_catalog_revision: ContentDigest,
    pub after_catalog_revision: ContentDigest,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact: Option<SkillArtifactRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub binding: Option<SkillSourceBinding>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_binding: Option<SkillSourceBinding>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rollback_of: Option<ReceiptId>,
    #[serde(default)]
    pub affected_resources: Vec<ResourceRef>,
    #[serde(default)]
    pub affected_workspaces: Vec<WorkspaceKey>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backup_id: Option<String>,
    pub status: SkillCatalogReceiptStatus,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillCatalogOperationReport {
    pub outcome: SkillCatalogOperationOutcome,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<SkillCatalogEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt: Option<SkillCatalogReceipt>,
    #[serde(default)]
    pub issues: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillCatalogRecoveryReport {
    pub inspected: usize,
    pub recovered: usize,
    pub compensated: usize,
    pub repair_required: usize,
    pub removed_staging: usize,
    #[serde(default)]
    pub diagnostics: Vec<String>,
}

impl SkillCatalogRecoveryReport {
    pub fn writable(&self) -> bool {
        self.repair_required == 0
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SkillCatalogExecutionError {
    #[error(transparent)]
    Plan(#[from] SkillCatalogPlanError),
    #[error(transparent)]
    Catalog(#[from] SkillCatalogError),
    #[error(transparent)]
    Artifact(#[from] SkillArtifactError),
    #[error("Skill catalog changed after preview")]
    CatalogChanged,
    #[error("Skill source changed after preview")]
    SourceChanged,
    #[error("Skill catalog mutation is locked by another process")]
    Locked,
    #[error("Skill catalog recovery requires manual repair")]
    RepairRequired,
    #[error("Skill catalog receipt cannot be rolled back")]
    InvalidReceipt,
    #[error("Skill catalog transaction I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("Skill catalog changed but its receipt could not be persisted: {0}")]
    ReceiptPending(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum CatalogJournalState {
    Prepared,
    Applying,
    Committed,
    Compensated,
    RepairRequired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CatalogJournal {
    schema_version: u32,
    instance_id: String,
    operation_id: String,
    state: CatalogJournalState,
    receipt: SkillCatalogReceipt,
}

pub fn apply_skill_catalog_plan(
    store: &SkillCatalogPlanStore,
    claim: &SkillCatalogPlanClaim,
) -> Result<SkillCatalogOperationReport, SkillCatalogExecutionError> {
    let now = Utc::now();
    let binding = store.binding(claim, now)?;
    let state = ExecutionState::open()?;
    ensure_recovery_writable(&state)?;
    let catalog_path =
        skill_catalog_path().map_err(|error| SkillCatalogError::Corrupt(error.to_string()))?;
    let operation_id = uuid::Uuid::new_v4().to_string();
    let mut lock_targets = vec![
        catalog_path,
        resource_catalog_lock_target(&state),
        state.ownership().display_path().to_path_buf(),
    ];
    lock_targets.extend(
        catalog_plan_resource_ids(&binding.view)
            .iter()
            .map(|resource_id| resource_lifecycle_lock_target(&state, resource_id)),
    );
    let _lock = TargetLockSet::acquire_for_ad_states(&lock_targets, &operation_id, &state)
        .map_err(lock_error)?;
    let current = load_skill_catalog_state_from(state.state())?;
    if current.revision != binding.view.expected_catalog_revision {
        return Err(SkillCatalogExecutionError::CatalogChanged);
    }
    if let Some(source) = &binding.source {
        let expected_revision = binding
            .view
            .binding
            .as_ref()
            .map(|value| value.source_revision.as_str())
            .or_else(|| {
                binding
                    .view
                    .artifact
                    .as_ref()
                    .map(|value| value.source_revision.as_str())
            });
        if let Some(expected) = expected_revision {
            if observe_skill_source_revision(source)? != expected {
                return Err(SkillCatalogExecutionError::SourceChanged);
            }
        }
    }
    if !skill_catalog_plan_references_are_current(&binding.view)? {
        return Err(SkillCatalogPlanError::RiskChanged.into());
    }
    let claimed = store.claim(claim, now)?;
    if is_no_change(&claimed) {
        return Ok(SkillCatalogOperationReport {
            outcome: SkillCatalogOperationOutcome::NoChange,
            source: current.document.entry(&claimed.view.source_id).cloned(),
            receipt: None,
            issues: Vec::new(),
        });
    }
    apply_claimed_plan(claimed, current, operation_id, &state)
}

fn catalog_plan_resource_ids(view: &SkillCatalogPlanView) -> std::collections::BTreeSet<String> {
    let mut ids = std::collections::BTreeSet::new();
    for binding in [view.binding.as_ref(), view.current_binding.as_ref()]
        .into_iter()
        .flatten()
    {
        if binding.resources.is_empty() {
            ids.extend(binding.skills.iter().map(|skill| {
                catalog_resource_id(&view.source_id, ResourceKind::Skills, &skill.logical_id)
            }));
        } else {
            ids.extend(binding.resources.iter().map(|resource| {
                catalog_resource_id(&view.source_id, resource.kind, &resource.install_id)
            }));
        }
    }
    for artifact in [view.artifact.as_ref(), view.current_artifact.as_ref()]
        .into_iter()
        .flatten()
    {
        ids.extend(artifact.skills.iter().map(|skill| {
            catalog_resource_id(&view.source_id, ResourceKind::Skills, &skill.logical_id)
        }));
    }
    ids
}

pub fn preview_rollback_skill_catalog_source(
    receipt_id: &ReceiptId,
    store: &SkillCatalogPlanStore,
) -> Result<SkillCatalogPlanView, SkillCatalogExecutionError> {
    let state = ExecutionState::open()?;
    ensure_recovery_writable(&state)?;
    let receipt = load_receipt(&state, receipt_id)?;
    if receipt.schema_version != RECEIPT_SCHEMA_VERSION
        || receipt.id != *receipt_id
        || receipt.action != SkillCatalogAction::Update
        || !matches!(
            receipt.status,
            SkillCatalogReceiptStatus::Complete | SkillCatalogReceiptStatus::Recovered
        )
    {
        return Err(SkillCatalogExecutionError::InvalidReceipt);
    }
    let current_binding = receipt
        .binding
        .as_ref()
        .filter(|binding| binding.source_type == SkillSourceType::Git)
        .ok_or(SkillCatalogExecutionError::InvalidReceipt)?;
    let target = receipt
        .previous_binding
        .clone()
        .filter(|binding| {
            binding.source_type == SkillSourceType::Git
                && binding.source_id == current_binding.source_id
                && binding.binding_id == current_binding.binding_id
                && binding.stable_root == current_binding.stable_root
        })
        .ok_or(SkillCatalogExecutionError::InvalidReceipt)?;
    let catalog = load_skill_catalog_state_from(state.state())?;
    if catalog.revision != receipt.after_catalog_revision {
        return Err(SkillCatalogExecutionError::CatalogChanged);
    }
    store
        .preview_rollback(
            receipt.id.clone(),
            &receipt.source_id,
            current_binding,
            target,
        )
        .map_err(Into::into)
}

fn apply_claimed_plan(
    mut plan: ClaimedSkillCatalogPlan,
    current: super::skill_catalog::SkillCatalogState,
    operation_id: String,
    state: &ExecutionState,
) -> Result<SkillCatalogOperationReport, SkillCatalogExecutionError> {
    let now = Utc::now();
    let mut document = current.document.clone();
    let artifact = plan.view.artifact.clone();
    let binding = plan.view.binding.clone();
    let source = mutate_document(&mut document, &plan, now)?;
    let after_bytes = document.render()?;
    let after_revision = ContentDigest::sha256(&after_bytes);
    let receipt_id = ReceiptId::from(format!("skill-catalog-receipt:{}", uuid::Uuid::new_v4()));
    let backup_id = persist_backup(state, &receipt_id, current.bytes.as_deref())?;
    let receipt = SkillCatalogReceipt {
        schema_version: RECEIPT_SCHEMA_VERSION,
        id: receipt_id,
        plan_id: plan.view.id.clone(),
        action: plan.view.action,
        source_id: plan.view.source_id.clone(),
        before_catalog_revision: current.revision.clone(),
        after_catalog_revision: after_revision.clone(),
        artifact,
        binding,
        previous_binding: plan.view.current_binding.clone(),
        rollback_of: plan.rollback_of.clone(),
        affected_resources: plan.view.affected_resources.clone(),
        affected_workspaces: plan.view.affected_workspaces.clone(),
        backup_id,
        status: SkillCatalogReceiptStatus::Complete,
        created_at: now,
    };
    let mut journal = CatalogJournal {
        schema_version: JOURNAL_SCHEMA_VERSION,
        instance_id: execution_instance_id().to_owned(),
        operation_id,
        state: CatalogJournalState::Prepared,
        receipt,
    };
    let journal_name = journal_name(&plan.view.id);
    persist_journal(state, &journal_name, &journal, false)?;
    journal.state = CatalogJournalState::Applying;
    persist_journal(state, &journal_name, &journal, true)?;
    let mut git_publication = None;
    if let Some(staged) = plan.staged_git.take() {
        match publish_staged_git_skill_source_binding(staged, plan.view.current_binding.as_ref()) {
            Ok((published, publication)) if plan.view.binding.as_ref() == Some(&published) => {
                git_publication = Some(publication);
            }
            Ok(_) => {
                return Err(SkillCatalogExecutionError::SourceChanged);
            }
            Err(error) => {
                journal.state = CatalogJournalState::Compensated;
                persist_journal(state, &journal_name, &journal, true)?;
                let mut receipt = journal.receipt.clone();
                receipt.status = SkillCatalogReceiptStatus::Compensated;
                persist_receipt(state, &receipt)?;
                return Ok(SkillCatalogOperationReport {
                    outcome: SkillCatalogOperationOutcome::Compensated,
                    source: None,
                    receipt: Some(receipt),
                    issues: vec![error.to_string()],
                });
            }
        }
    }
    if plan.rollback_of.is_some() {
        let rollback_result = plan
            .view
            .binding
            .as_ref()
            .zip(plan.view.current_binding.as_ref())
            .ok_or(SkillCatalogExecutionError::InvalidReceipt)
            .and_then(|(target, current)| {
                switch_git_skill_source_binding(target, current).map_err(Into::into)
            });
        match rollback_result {
            Ok(publication) => git_publication = Some(publication),
            Err(error) => {
                journal.state = CatalogJournalState::Compensated;
                persist_journal(state, &journal_name, &journal, true)?;
                let mut receipt = journal.receipt.clone();
                receipt.status = SkillCatalogReceiptStatus::Compensated;
                persist_receipt(state, &receipt)?;
                return Ok(SkillCatalogOperationReport {
                    outcome: SkillCatalogOperationOutcome::Compensated,
                    source: None,
                    receipt: Some(receipt),
                    issues: vec![error.to_string()],
                });
            }
        }
    }
    if let Err(error) = state
        .state()
        .write_atomic("skill_catalog.json", &after_bytes)
    {
        let compensation_issue = git_publication
            .take()
            .and_then(|publication| publication.compensate().err())
            .map(|error| error.to_string());
        journal.state = CatalogJournalState::Compensated;
        persist_journal(state, &journal_name, &journal, true)?;
        let mut receipt = journal.receipt.clone();
        receipt.status = SkillCatalogReceiptStatus::Compensated;
        persist_receipt(state, &receipt)?;
        return Ok(SkillCatalogOperationReport {
            outcome: SkillCatalogOperationOutcome::Compensated,
            source: None,
            receipt: Some(receipt),
            issues: std::iter::once(error.to_string())
                .chain(compensation_issue)
                .collect(),
        });
    }
    let projected = load_skill_catalog_state_from(state.state())?.snapshot();
    if let Err(error) = persist_resource_catalog_projection(state.state(), &projected) {
        let catalog_compensation = match current.bytes.as_deref() {
            Some(bytes) => state.state().write_atomic("skill_catalog.json", bytes),
            None => state.state().remove("skill_catalog.json"),
        }
        .err()
        .map(|error| error.to_string());
        let source_compensation = git_publication
            .take()
            .and_then(|publication| publication.compensate().err())
            .map(|error| error.to_string());
        journal.state = CatalogJournalState::Compensated;
        persist_journal(state, &journal_name, &journal, true)?;
        let mut receipt = journal.receipt.clone();
        receipt.status = SkillCatalogReceiptStatus::Compensated;
        persist_receipt(state, &receipt)?;
        return Ok(SkillCatalogOperationReport {
            outcome: SkillCatalogOperationOutcome::Compensated,
            source: None,
            receipt: Some(receipt),
            issues: std::iter::once(error.to_string())
                .chain(catalog_compensation)
                .chain(source_compensation)
                .collect(),
        });
    }
    if let Some(publication) = git_publication.take() {
        publication.commit();
    }
    if let Err(error) = persist_receipt(state, &journal.receipt) {
        return Err(SkillCatalogExecutionError::ReceiptPending(
            error.to_string(),
        ));
    }
    journal.state = CatalogJournalState::Committed;
    persist_journal(state, &journal_name, &journal, true)?;
    Ok(SkillCatalogOperationReport {
        outcome: SkillCatalogOperationOutcome::Changed,
        source,
        receipt: Some(journal.receipt),
        issues: Vec::new(),
    })
}

fn mutate_document(
    document: &mut super::skill_catalog::SkillCatalogDocument,
    plan: &ClaimedSkillCatalogPlan,
    now: DateTime<Utc>,
) -> Result<Option<SkillCatalogEntry>, SkillCatalogExecutionError> {
    match plan.view.action {
        SkillCatalogAction::Add => {
            let request = plan
                .request
                .as_ref()
                .ok_or(SkillCatalogPlanError::InvalidPlan)?;
            if let Some(binding) = plan.view.binding.clone() {
                document
                    .add_binding(plan.view.source_id.clone(), request, binding, now)
                    .map(Some)
                    .map_err(Into::into)
            } else {
                let artifact = plan
                    .view
                    .artifact
                    .clone()
                    .ok_or(SkillCatalogPlanError::InvalidPlan)?;
                document
                    .add(plan.view.source_id.clone(), request, artifact, now)
                    .map(Some)
                    .map_err(Into::into)
            }
        }
        SkillCatalogAction::Update => {
            if let Some(binding) = plan.view.binding.clone() {
                document
                    .update_binding(&plan.view.source_id, binding, now)
                    .map(Some)
                    .map_err(Into::into)
            } else {
                let artifact = plan
                    .view
                    .artifact
                    .clone()
                    .ok_or(SkillCatalogPlanError::InvalidPlan)?;
                document
                    .update_artifact(&plan.view.source_id, artifact, now)
                    .map(Some)
                    .map_err(Into::into)
            }
        }
        SkillCatalogAction::Remove => document
            .remove(&plan.view.source_id)
            .map(|_| None)
            .map_err(Into::into),
    }
}

fn is_no_change(plan: &ClaimedSkillCatalogPlan) -> bool {
    plan.view.action == SkillCatalogAction::Update
        && ((plan.view.binding.is_some() && plan.view.binding == plan.view.current_binding)
            || (plan.view.artifact.is_some() && plan.view.artifact == plan.view.current_artifact))
}

fn persist_backup(
    state: &ExecutionState,
    receipt_id: &ReceiptId,
    bytes: Option<&[u8]>,
) -> Result<Option<String>, std::io::Error> {
    let Some(bytes) = bytes else { return Ok(None) };
    let name = format!("{}.json", opaque_name(receipt_id.as_str()));
    state
        .skill_catalog_backups()
        .write_atomic_new(&name, bytes)?;
    Ok(Some(name))
}

fn persist_journal(
    state: &ExecutionState,
    name: &str,
    journal: &CatalogJournal,
    replace: bool,
) -> Result<(), std::io::Error> {
    let bytes = serde_json::to_vec_pretty(journal).map_err(std::io::Error::other)?;
    if replace {
        state.skill_catalog_journals().write_atomic(name, &bytes)
    } else {
        state
            .skill_catalog_journals()
            .write_atomic_new(name, &bytes)
    }
}

fn persist_receipt(
    state: &ExecutionState,
    receipt: &SkillCatalogReceipt,
) -> Result<(), std::io::Error> {
    let name = format!("{}.json", opaque_name(receipt.id.as_str()));
    let bytes = serde_json::to_vec_pretty(receipt).map_err(std::io::Error::other)?;
    match state
        .skill_catalog_history()
        .write_atomic_new(&name, &bytes)
    {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            if state.skill_catalog_history().read(&name)? == bytes {
                Ok(())
            } else {
                Err(std::io::Error::new(
                    std::io::ErrorKind::AlreadyExists,
                    "Skill catalog receipt identity collision",
                ))
            }
        }
        Err(error) => Err(error),
    }
}

fn load_receipt(
    state: &ExecutionState,
    receipt_id: &ReceiptId,
) -> Result<SkillCatalogReceipt, SkillCatalogExecutionError> {
    let name = format!("{}.json", opaque_name(receipt_id.as_str()));
    let bytes = state.skill_catalog_history().read(&name)?;
    serde_json::from_slice(&bytes).map_err(|_| SkillCatalogExecutionError::InvalidReceipt)
}

fn journal_name(plan_id: &PlanId) -> String {
    format!("{}.json", opaque_name(plan_id.as_str()))
}

fn opaque_name(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}

fn lock_error(error: std::io::Error) -> SkillCatalogExecutionError {
    if error.kind() == std::io::ErrorKind::WouldBlock {
        SkillCatalogExecutionError::Locked
    } else {
        SkillCatalogExecutionError::Io(error)
    }
}

pub fn recover_skill_catalog_state(
) -> Result<SkillCatalogRecoveryReport, SkillCatalogExecutionError> {
    let state = ExecutionState::open()?;
    let catalog_path =
        skill_catalog_path().map_err(|error| SkillCatalogError::Corrupt(error.to_string()))?;
    let recovery_id = format!("skill-catalog-recovery:{}", uuid::Uuid::new_v4());
    let _lock = TargetLockSet::acquire_for_ad_states(
        &[
            catalog_path,
            resource_catalog_lock_target(&state),
            state.ownership().display_path().to_path_buf(),
        ],
        &recovery_id,
        &state,
    )
    .map_err(lock_error)?;
    let mut report = SkillCatalogRecoveryReport {
        inspected: 0,
        recovered: 0,
        compensated: 0,
        repair_required: 0,
        removed_staging: 0,
        diagnostics: Vec::new(),
    };
    for name in state.skill_catalog_journals().entry_names()? {
        let Some(name) = name.to_str() else {
            report.repair_required += 1;
            report.diagnostics.push("non-UTF-8 catalog journal".into());
            continue;
        };
        if !name.ends_with(".json") {
            report.repair_required += 1;
            report
                .diagnostics
                .push(format!("unexpected catalog journal entry: {name}"));
            continue;
        }
        report.inspected += 1;
        let bytes = state.skill_catalog_journals().read(name)?;
        let mut journal = match serde_json::from_slice::<CatalogJournal>(&bytes) {
            Ok(journal) if journal.schema_version == JOURNAL_SCHEMA_VERSION => journal,
            Ok(_) => {
                report.repair_required += 1;
                report
                    .diagnostics
                    .push(format!("unsupported catalog journal: {name}"));
                continue;
            }
            Err(error) => {
                report.repair_required += 1;
                report
                    .diagnostics
                    .push(format!("invalid catalog journal {name}: {error}"));
                continue;
            }
        };
        match journal.state {
            CatalogJournalState::Committed | CatalogJournalState::Compensated => continue,
            CatalogJournalState::RepairRequired => {
                report.repair_required += 1;
                report
                    .diagnostics
                    .push(format!("catalog journal requires repair: {name}"));
                continue;
            }
            CatalogJournalState::Prepared | CatalogJournalState::Applying => {}
        }
        let current = load_skill_catalog_state_from(state.state())?;
        if current.revision == journal.receipt.after_catalog_revision {
            if let Some(binding) = &journal.receipt.binding {
                if let Err(error) = reconcile_git_skill_source_current(
                    binding,
                    journal.receipt.previous_binding.as_ref(),
                    true,
                ) {
                    journal.state = CatalogJournalState::RepairRequired;
                    persist_journal(&state, name, &journal, true)?;
                    report.repair_required += 1;
                    report.diagnostics.push(format!(
                        "catalog current differs from committed journal {name}: {error}"
                    ));
                    continue;
                }
            }
            let mut receipt = journal.receipt.clone();
            receipt.status = SkillCatalogReceiptStatus::Recovered;
            persist_receipt(&state, &receipt)?;
            journal.receipt = receipt;
            journal.state = CatalogJournalState::Committed;
            persist_journal(&state, name, &journal, true)?;
            report.recovered += 1;
        } else if current.revision == journal.receipt.before_catalog_revision {
            if let Some(binding) = &journal.receipt.binding {
                if let Err(error) = reconcile_git_skill_source_current(
                    binding,
                    journal.receipt.previous_binding.as_ref(),
                    false,
                ) {
                    journal.state = CatalogJournalState::RepairRequired;
                    persist_journal(&state, name, &journal, true)?;
                    report.repair_required += 1;
                    report.diagnostics.push(format!(
                        "catalog current cannot be compensated for journal {name}: {error}"
                    ));
                    continue;
                }
            }
            let mut receipt = journal.receipt.clone();
            receipt.status = SkillCatalogReceiptStatus::Compensated;
            persist_receipt(&state, &receipt)?;
            journal.receipt = receipt;
            journal.state = CatalogJournalState::Compensated;
            persist_journal(&state, name, &journal, true)?;
            report.compensated += 1;
        } else {
            journal.state = CatalogJournalState::RepairRequired;
            persist_journal(&state, name, &journal, true)?;
            report.repair_required += 1;
            report.diagnostics.push(format!(
                "catalog revision differs from both sides of journal {name}"
            ));
        }
    }
    if report.writable() {
        report.removed_staging =
            cleanup_unpublished_skill_staging(&std::collections::BTreeSet::new())?;
    }
    Ok(report)
}

fn ensure_recovery_writable(state: &ExecutionState) -> Result<(), SkillCatalogExecutionError> {
    for name in state.skill_catalog_journals().entry_names()? {
        let Some(name) = name.to_str() else {
            return Err(SkillCatalogExecutionError::RepairRequired);
        };
        let bytes = state.skill_catalog_journals().read(name)?;
        let journal: CatalogJournal = serde_json::from_slice(&bytes)
            .map_err(|_| SkillCatalogExecutionError::RepairRequired)?;
        if journal.schema_version != JOURNAL_SCHEMA_VERSION
            || matches!(
                journal.state,
                CatalogJournalState::Prepared
                    | CatalogJournalState::Applying
                    | CatalogJournalState::RepairRequired
            )
        {
            return Err(SkillCatalogExecutionError::RepairRequired);
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "skill_catalog_execution_tests.rs"]
mod tests;
