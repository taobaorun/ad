use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::fs::paths::{project_skills_dir, skill_migration_archive_dir};

use super::execution_confinement::{capture_project_root_identity, ConfinedTarget};
use super::execution_fs::TargetState;
use super::execution_lock::{execution_instance_id, TargetLockSet};
use super::execution_state::ExecutionState;
use super::skill_legacy_inventory::operation_receipt_proves_record;
use super::{
    inspect_legacy_skill_inventory, validate_ownership_artifact, validate_ownership_record,
    AgentContext, ContentDigest, LegacyProjectSkillView, LegacySkillArchiveMarker,
    LegacySkillArchiveStatus, LegacySkillInventoryError, LegacySkillMigrationStatus, PlanId,
    ReceiptId, ResourceStateKind, RiskFingerprint,
};

const MIGRATION_SCHEMA_VERSION: u32 = 1;
const PLAN_TTL_MINUTES: i64 = 5;
const MAX_STORED_PLANS: usize = 32;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacySkillMigrationPlanView {
    pub schema_version: u32,
    pub id: PlanId,
    pub project_path: String,
    pub canonical_project_path: String,
    pub state_id: String,
    pub state_digest: ContentDigest,
    #[serde(default)]
    pub migration_receipt_ids: Vec<ReceiptId>,
    pub confirmation_required: bool,
    pub risk_fingerprint: RiskFingerprint,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LegacySkillMigrationPlanClaim {
    pub plan_id: PlanId,
    pub risk_fingerprint: RiskFingerprint,
    pub confirmed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LegacySkillMigrationReceiptStatus {
    Complete,
    Recovered,
    Compensated,
    Restored,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LegacySkillMigrationReceipt {
    pub schema_version: u32,
    pub id: ReceiptId,
    pub plan_id: PlanId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_receipt_id: Option<ReceiptId>,
    pub archive_id: String,
    pub original_state_id: String,
    pub project_path: String,
    pub canonical_project_path: String,
    pub state_digest: ContentDigest,
    #[serde(default)]
    pub migration_receipt_ids: Vec<ReceiptId>,
    pub status: LegacySkillMigrationReceiptStatus,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LegacySkillMigrationOutcome {
    Archived,
    Restored,
    Compensated,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacySkillMigrationReport {
    pub outcome: LegacySkillMigrationOutcome,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt: Option<LegacySkillMigrationReceipt>,
    #[serde(default)]
    pub issues: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacySkillMigrationRecoveryReport {
    pub inspected: usize,
    pub recovered: usize,
    pub compensated: usize,
    pub repair_required: usize,
    #[serde(default)]
    pub diagnostics: Vec<String>,
}

impl LegacySkillMigrationRecoveryReport {
    pub fn writable(&self) -> bool {
        self.repair_required == 0
    }
}

#[derive(Debug, thiserror::Error)]
pub enum LegacySkillMigrationError {
    #[error(transparent)]
    Inventory(#[from] LegacySkillInventoryError),
    #[error("legacy project Skill state is not ready to archive")]
    NotReady,
    #[error("legacy project Skill migration plan is unknown or already consumed")]
    InvalidPlan,
    #[error("legacy project Skill migration plan expired")]
    Expired,
    #[error("legacy project Skill migration confirmation is required")]
    ConfirmationRequired,
    #[error("legacy project Skill migration risk changed")]
    RiskChanged,
    #[error("legacy project Skill state changed after preview")]
    StateChanged,
    #[error("legacy project Skill migration is locked by another process")]
    Locked,
    #[error("legacy project Skill migration recovery requires manual repair")]
    RepairRequired,
    #[error("legacy project Skill migration receipt was not found")]
    ReceiptNotFound,
    #[error("legacy project Skill migration receipt is not rollback eligible")]
    RollbackUnavailable,
    #[error("legacy project Skill migration I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("legacy project Skill state moved but receipt persistence is pending: {0}")]
    ReceiptPending(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum MigrationOperation {
    Archive,
    Rollback,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum MigrationJournalState {
    Prepared,
    Applying,
    Committed,
    Compensated,
    RolledBack,
    RepairRequired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct MigrationJournal {
    schema_version: u32,
    instance_id: String,
    operation_id: String,
    operation: MigrationOperation,
    state: MigrationJournalState,
    receipt: LegacySkillMigrationReceipt,
    marker: LegacySkillArchiveMarker,
}

#[derive(Default)]
pub struct LegacySkillMigrationPlanStore {
    plans: Mutex<HashMap<PlanId, LegacySkillMigrationPlanView>>,
}

impl LegacySkillMigrationPlanStore {
    pub fn cancel(&self, plan_id: &PlanId) -> Result<bool, LegacySkillMigrationError> {
        let mut plans = self
            .plans
            .lock()
            .map_err(|_| LegacySkillMigrationError::InvalidPlan)?;
        Ok(plans.remove(plan_id).is_some())
    }

    fn insert(&self, plan: LegacySkillMigrationPlanView) -> Result<(), LegacySkillMigrationError> {
        let mut plans = self
            .plans
            .lock()
            .map_err(|_| LegacySkillMigrationError::InvalidPlan)?;
        prune_plans(&mut plans, Utc::now());
        if plans.len() >= MAX_STORED_PLANS {
            if let Some(oldest) = plans
                .values()
                .min_by_key(|candidate| candidate.expires_at)
                .map(|candidate| candidate.id.clone())
            {
                plans.remove(&oldest);
            }
        }
        plans.insert(plan.id.clone(), plan);
        Ok(())
    }

    fn binding(
        &self,
        claim: &LegacySkillMigrationPlanClaim,
        now: DateTime<Utc>,
    ) -> Result<LegacySkillMigrationPlanView, LegacySkillMigrationError> {
        let mut plans = self
            .plans
            .lock()
            .map_err(|_| LegacySkillMigrationError::InvalidPlan)?;
        prune_plans(&mut plans, now);
        let plan = plans
            .get(&claim.plan_id)
            .ok_or(LegacySkillMigrationError::InvalidPlan)?;
        validate_claim(plan, claim, now)?;
        Ok(plan.clone())
    }

    fn claim(
        &self,
        claim: &LegacySkillMigrationPlanClaim,
        now: DateTime<Utc>,
    ) -> Result<LegacySkillMigrationPlanView, LegacySkillMigrationError> {
        let mut plans = self
            .plans
            .lock()
            .map_err(|_| LegacySkillMigrationError::InvalidPlan)?;
        prune_plans(&mut plans, now);
        let plan = plans
            .get(&claim.plan_id)
            .ok_or(LegacySkillMigrationError::InvalidPlan)?;
        validate_claim(plan, claim, now)?;
        plans
            .remove(&claim.plan_id)
            .ok_or(LegacySkillMigrationError::InvalidPlan)
    }
}

pub fn preview_legacy_project_skill_migration(
    project_path: &Path,
    store: &LegacySkillMigrationPlanStore,
) -> Result<LegacySkillMigrationPlanView, LegacySkillMigrationError> {
    let project = ready_project(project_path)?;
    let now = Utc::now();
    let mut migration_receipt_ids = project
        .links
        .iter()
        .filter_map(|link| link.migration_receipt_id.clone())
        .collect::<Vec<_>>();
    migration_receipt_ids.sort();
    migration_receipt_ids.dedup();
    let canonical_project_path = project
        .canonical_project_path
        .clone()
        .ok_or(LegacySkillMigrationError::NotReady)?;
    let id = PlanId::from(format!(
        "legacy-skill-migration-plan:{}",
        uuid::Uuid::new_v4()
    ));
    let risk_fingerprint = migration_risk(
        &canonical_project_path,
        &project.state_id,
        &project.state_digest,
        &migration_receipt_ids,
        &project,
    );
    let plan = LegacySkillMigrationPlanView {
        schema_version: MIGRATION_SCHEMA_VERSION,
        id,
        project_path: project.project_path,
        canonical_project_path,
        state_id: project.state_id,
        state_digest: project.state_digest,
        migration_receipt_ids,
        confirmation_required: true,
        risk_fingerprint,
        expires_at: now + Duration::minutes(PLAN_TTL_MINUTES),
    };
    store.insert(plan.clone())?;
    Ok(plan)
}

pub fn apply_legacy_project_skill_migration(
    store: &LegacySkillMigrationPlanStore,
    claim: &LegacySkillMigrationPlanClaim,
) -> Result<LegacySkillMigrationReport, LegacySkillMigrationError> {
    let now = Utc::now();
    let binding = store.binding(claim, now)?;
    let state = ExecutionState::open()?;
    ensure_recovery_writable(&state)?;
    let archive_id = archive_id(&binding);
    let archive_name = archive_name(&archive_id);
    let source_path = project_skills_dir()
        .map_err(|error| std::io::Error::other(error.to_string()))?
        .join(&binding.state_id);
    let archive_path = skill_migration_archive_dir()
        .map_err(|error| std::io::Error::other(error.to_string()))?
        .join(&archive_name);
    let operation_id = format!("legacy-skill-migration:{}", uuid::Uuid::new_v4());
    let current = validate_current_state(&binding)?;
    let mut lock_targets = vec![source_path, archive_path];
    lock_targets.extend(managed_link_targets(&current)?);
    let _locks = TargetLockSet::acquire_for_ad_states(&lock_targets, &operation_id, &state)
        .map_err(lock_error)?;
    let current = validate_current_state(&binding)?;
    let _bound_links = bind_managed_links(&state, &current)?;
    let plan = store.claim(claim, now)?;
    let bytes = state.legacy_project_skills().read(&plan.state_id)?;
    if ContentDigest::sha256(&bytes) != plan.state_digest {
        return Err(LegacySkillMigrationError::StateChanged);
    }
    if entry_exists(state.skill_migration_archive(), &archive_name)? {
        return Err(LegacySkillMigrationError::StateChanged);
    }
    let replace_marker = restored_marker_matches_plan(&state, &plan, &archive_name)?;

    let receipt = LegacySkillMigrationReceipt {
        schema_version: MIGRATION_SCHEMA_VERSION,
        id: ReceiptId::from(format!(
            "legacy-skill-migration-receipt:{}",
            uuid::Uuid::new_v4()
        )),
        plan_id: plan.id.clone(),
        parent_receipt_id: None,
        archive_id: archive_id.clone(),
        original_state_id: plan.state_id.clone(),
        project_path: plan.project_path.clone(),
        canonical_project_path: plan.canonical_project_path.clone(),
        state_digest: plan.state_digest.clone(),
        migration_receipt_ids: plan.migration_receipt_ids.clone(),
        status: LegacySkillMigrationReceiptStatus::Complete,
        created_at: now,
    };
    let marker = marker(&receipt, &archive_name, LegacySkillArchiveStatus::Archived);
    let mut journal = MigrationJournal {
        schema_version: MIGRATION_SCHEMA_VERSION,
        instance_id: execution_instance_id().to_owned(),
        operation_id,
        operation: MigrationOperation::Archive,
        state: MigrationJournalState::Prepared,
        receipt,
        marker,
    };
    let journal_name = journal_name(&plan.id);
    persist_journal(&state, &journal_name, &journal, false)?;
    journal.state = MigrationJournalState::Applying;
    persist_journal(&state, &journal_name, &journal, true)?;
    state.legacy_project_skills().rename_entry_to(
        &plan.state_id,
        state.skill_migration_archive(),
        &archive_name,
    )?;
    if !digest_matches(
        state.skill_migration_archive(),
        &archive_name,
        &journal.receipt.state_digest,
    )? {
        state.skill_migration_archive().rename_entry_to(
            &archive_name,
            state.legacy_project_skills(),
            &plan.state_id,
        )?;
        journal.receipt.status = LegacySkillMigrationReceiptStatus::Compensated;
        persist_receipt(&state, &journal.receipt)?;
        journal.state = MigrationJournalState::Compensated;
        persist_journal(&state, &journal_name, &journal, true)?;
        return Ok(LegacySkillMigrationReport {
            outcome: LegacySkillMigrationOutcome::Compensated,
            receipt: Some(journal.receipt),
            issues: vec!["legacy_state_changed_during_archive".into()],
        });
    }
    if let Err(error) = finalize_archive(&state, &journal, replace_marker) {
        return Err(LegacySkillMigrationError::ReceiptPending(error.to_string()));
    }
    journal.state = MigrationJournalState::Committed;
    persist_journal(&state, &journal_name, &journal, true)?;
    Ok(LegacySkillMigrationReport {
        outcome: LegacySkillMigrationOutcome::Archived,
        receipt: Some(journal.receipt),
        issues: Vec::new(),
    })
}

pub fn rollback_legacy_project_skill_migration(
    receipt_id: &ReceiptId,
) -> Result<LegacySkillMigrationReport, LegacySkillMigrationError> {
    let state = ExecutionState::open()?;
    ensure_recovery_writable(&state)?;
    let original = read_receipt(&state, receipt_id)?;
    if original.parent_receipt_id.is_some()
        || !matches!(
            original.status,
            LegacySkillMigrationReceiptStatus::Complete
                | LegacySkillMigrationReceiptStatus::Recovered
        )
    {
        return Err(LegacySkillMigrationError::RollbackUnavailable);
    }
    let archive_name = archive_name(&original.archive_id);
    let source_path = project_skills_dir()
        .map_err(|error| std::io::Error::other(error.to_string()))?
        .join(&original.original_state_id);
    let archive_path = skill_migration_archive_dir()
        .map_err(|error| std::io::Error::other(error.to_string()))?
        .join(&archive_name);
    let operation_id = format!("legacy-skill-migration-rollback:{}", uuid::Uuid::new_v4());
    let _locks =
        TargetLockSet::acquire_for_ad_states(&[source_path, archive_path], &operation_id, &state)
            .map_err(lock_error)?;
    validate_archived_marker(&state, &original, &archive_name)?;
    if entry_exists(state.legacy_project_skills(), &original.original_state_id)? {
        return Err(LegacySkillMigrationError::RollbackUnavailable);
    }
    let bytes = state.skill_migration_archive().read(&archive_name)?;
    if ContentDigest::sha256(&bytes) != original.state_digest {
        return Err(LegacySkillMigrationError::StateChanged);
    }
    let rollback_plan_id = PlanId::from(format!(
        "legacy-skill-migration-rollback:{}",
        uuid::Uuid::new_v4()
    ));
    let receipt = LegacySkillMigrationReceipt {
        schema_version: MIGRATION_SCHEMA_VERSION,
        id: ReceiptId::from(format!(
            "legacy-skill-migration-receipt:{}",
            uuid::Uuid::new_v4()
        )),
        plan_id: rollback_plan_id.clone(),
        parent_receipt_id: Some(original.id.clone()),
        archive_id: original.archive_id.clone(),
        original_state_id: original.original_state_id.clone(),
        project_path: original.project_path.clone(),
        canonical_project_path: original.canonical_project_path.clone(),
        state_digest: original.state_digest.clone(),
        migration_receipt_ids: original.migration_receipt_ids.clone(),
        status: LegacySkillMigrationReceiptStatus::Restored,
        created_at: Utc::now(),
    };
    let marker = marker(&original, &archive_name, LegacySkillArchiveStatus::Restored);
    let mut journal = MigrationJournal {
        schema_version: MIGRATION_SCHEMA_VERSION,
        instance_id: execution_instance_id().to_owned(),
        operation_id,
        operation: MigrationOperation::Rollback,
        state: MigrationJournalState::Prepared,
        receipt,
        marker,
    };
    let journal_name = journal_name(&rollback_plan_id);
    persist_journal(&state, &journal_name, &journal, false)?;
    journal.state = MigrationJournalState::Applying;
    persist_journal(&state, &journal_name, &journal, true)?;
    state.skill_migration_archive().rename_entry_to(
        &archive_name,
        state.legacy_project_skills(),
        &original.original_state_id,
    )?;
    if !digest_matches(
        state.legacy_project_skills(),
        &original.original_state_id,
        &journal.receipt.state_digest,
    )? {
        state.legacy_project_skills().rename_entry_to(
            &original.original_state_id,
            state.skill_migration_archive(),
            &archive_name,
        )?;
        journal.receipt.status = LegacySkillMigrationReceiptStatus::Compensated;
        persist_receipt(&state, &journal.receipt)?;
        journal.state = MigrationJournalState::Compensated;
        persist_journal(&state, &journal_name, &journal, true)?;
        return Ok(LegacySkillMigrationReport {
            outcome: LegacySkillMigrationOutcome::Compensated,
            receipt: Some(journal.receipt),
            issues: vec!["legacy_archive_changed_during_restore".into()],
        });
    }
    if let Err(error) = finalize_rollback(&state, &journal) {
        return Err(LegacySkillMigrationError::ReceiptPending(error.to_string()));
    }
    journal.state = MigrationJournalState::RolledBack;
    persist_journal(&state, &journal_name, &journal, true)?;
    Ok(LegacySkillMigrationReport {
        outcome: LegacySkillMigrationOutcome::Restored,
        receipt: Some(journal.receipt),
        issues: Vec::new(),
    })
}

pub fn recover_legacy_skill_migrations(
) -> Result<LegacySkillMigrationRecoveryReport, LegacySkillMigrationError> {
    let state = ExecutionState::open()?;
    let mut report = LegacySkillMigrationRecoveryReport {
        inspected: 0,
        recovered: 0,
        compensated: 0,
        repair_required: 0,
        diagnostics: Vec::new(),
    };
    for name in state.skill_migration_journals().entry_names()? {
        let Some(name) = name.to_str() else {
            mark_repair(&mut report, "non-UTF-8 Skill migration journal");
            continue;
        };
        if !name.ends_with(".json") {
            mark_repair(
                &mut report,
                format!("unexpected Skill migration journal: {name}"),
            );
            continue;
        }
        report.inspected += 1;
        let bytes = state.skill_migration_journals().read(name)?;
        let mut journal = match serde_json::from_slice::<MigrationJournal>(&bytes) {
            Ok(journal) if journal.schema_version == MIGRATION_SCHEMA_VERSION => journal,
            _ => {
                mark_repair(
                    &mut report,
                    format!("invalid Skill migration journal: {name}"),
                );
                continue;
            }
        };
        if matches!(
            journal.state,
            MigrationJournalState::Committed
                | MigrationJournalState::Compensated
                | MigrationJournalState::RolledBack
        ) {
            continue;
        }
        if journal.state == MigrationJournalState::RepairRequired {
            mark_repair(
                &mut report,
                format!("Skill migration journal requires repair: {name}"),
            );
            continue;
        }
        let archive_name = archive_name(&journal.receipt.archive_id);
        let source_path = project_skills_dir()
            .map_err(|error| std::io::Error::other(error.to_string()))?
            .join(&journal.receipt.original_state_id);
        let archive_path = skill_migration_archive_dir()
            .map_err(|error| std::io::Error::other(error.to_string()))?
            .join(&archive_name);
        let recovery_id = format!("legacy-skill-migration-recovery:{}", uuid::Uuid::new_v4());
        let _locks = TargetLockSet::acquire_for_ad_states(
            &[source_path, archive_path],
            &recovery_id,
            &state,
        )
        .map_err(lock_error)?;
        let source_exists = entry_exists(
            state.legacy_project_skills(),
            &journal.receipt.original_state_id,
        )?;
        let archive_exists = entry_exists(state.skill_migration_archive(), &archive_name)?;
        match (journal.operation, source_exists, archive_exists) {
            (MigrationOperation::Archive, false, true) => {
                if !digest_matches(
                    state.skill_migration_archive(),
                    &archive_name,
                    &journal.receipt.state_digest,
                )? {
                    state.skill_migration_archive().rename_entry_to(
                        &archive_name,
                        state.legacy_project_skills(),
                        &journal.receipt.original_state_id,
                    )?;
                    compensate_recovery(&state, name, &mut journal, &mut report)?;
                    continue;
                }
                if !migration_receipt_persisted(&state, &journal.receipt)? {
                    journal.receipt.status = LegacySkillMigrationReceiptStatus::Recovered;
                }
                journal.marker = marker(
                    &journal.receipt,
                    &archive_name,
                    LegacySkillArchiveStatus::Archived,
                );
                finalize_archive(&state, &journal, true)?;
                journal.state = MigrationJournalState::Committed;
                persist_journal(&state, name, &journal, true)?;
                report.recovered += 1;
            }
            (MigrationOperation::Rollback, true, false) => {
                if !digest_matches(
                    state.legacy_project_skills(),
                    &journal.receipt.original_state_id,
                    &journal.receipt.state_digest,
                )? {
                    state.legacy_project_skills().rename_entry_to(
                        &journal.receipt.original_state_id,
                        state.skill_migration_archive(),
                        &archive_name,
                    )?;
                    compensate_recovery(&state, name, &mut journal, &mut report)?;
                    continue;
                }
                journal.receipt.status = LegacySkillMigrationReceiptStatus::Restored;
                finalize_rollback(&state, &journal)?;
                journal.state = MigrationJournalState::RolledBack;
                persist_journal(&state, name, &journal, true)?;
                report.recovered += 1;
            }
            (_, true, false) | (MigrationOperation::Rollback, false, true) => {
                compensate_recovery(&state, name, &mut journal, &mut report)?;
            }
            _ => {
                journal.state = MigrationJournalState::RepairRequired;
                persist_journal(&state, name, &journal, true)?;
                mark_repair(
                    &mut report,
                    format!("Skill migration state is ambiguous for journal: {name}"),
                );
            }
        }
    }
    Ok(report)
}

fn ready_project(project_path: &Path) -> Result<LegacyProjectSkillView, LegacySkillMigrationError> {
    let canonical =
        std::fs::canonicalize(project_path).map_err(|_| LegacySkillMigrationError::NotReady)?;
    let inventory = inspect_legacy_skill_inventory()?;
    let mut matches = inventory.projects.into_iter().filter(|project| {
        project.canonical_project_path.as_deref() == Some(canonical.to_string_lossy().as_ref())
    });
    let project = matches.next().ok_or(LegacySkillMigrationError::NotReady)?;
    if matches.next().is_some()
        || project.migration_status != LegacySkillMigrationStatus::ReadyToArchive
    {
        return Err(LegacySkillMigrationError::NotReady);
    }
    Ok(project)
}

fn validate_current_state(
    plan: &LegacySkillMigrationPlanView,
) -> Result<LegacyProjectSkillView, LegacySkillMigrationError> {
    let project = ready_project(Path::new(&plan.canonical_project_path))?;
    let mut receipts = project
        .links
        .iter()
        .filter_map(|link| link.migration_receipt_id.clone())
        .collect::<Vec<_>>();
    receipts.sort();
    receipts.dedup();
    let risk = migration_risk(
        &plan.canonical_project_path,
        &plan.state_id,
        &plan.state_digest,
        &receipts,
        &project,
    );
    if project.state_id != plan.state_id
        || project.state_digest != plan.state_digest
        || receipts != plan.migration_receipt_ids
        || project.canonical_project_path.as_deref() != Some(plan.canonical_project_path.as_str())
        || risk != plan.risk_fingerprint
    {
        return Err(LegacySkillMigrationError::StateChanged);
    }
    Ok(project)
}

fn validate_claim(
    plan: &LegacySkillMigrationPlanView,
    claim: &LegacySkillMigrationPlanClaim,
    now: DateTime<Utc>,
) -> Result<(), LegacySkillMigrationError> {
    if now >= plan.expires_at {
        return Err(LegacySkillMigrationError::Expired);
    }
    if !claim.confirmed {
        return Err(LegacySkillMigrationError::ConfirmationRequired);
    }
    if claim.risk_fingerprint != plan.risk_fingerprint {
        return Err(LegacySkillMigrationError::RiskChanged);
    }
    Ok(())
}

fn prune_plans(plans: &mut HashMap<PlanId, LegacySkillMigrationPlanView>, now: DateTime<Utc>) {
    plans.retain(|_, plan| plan.expires_at > now);
}

fn migration_risk(
    canonical_project_path: &str,
    state_id: &str,
    state_digest: &ContentDigest,
    receipts: &[ReceiptId],
    project: &LegacyProjectSkillView,
) -> RiskFingerprint {
    let links = project
        .links
        .iter()
        .map(|link| {
            serde_json::json!({
                "logicalId": link.logical_id,
                "sourceId": link.source_id,
                "targetKind": link.target_kind,
                "migrationReceiptId": link.migration_receipt_id,
                "health": link.health,
            })
        })
        .collect::<Vec<_>>();
    let bytes = serde_json::to_vec(&serde_json::json!({
        "schemaVersion": MIGRATION_SCHEMA_VERSION,
        "canonicalProjectPath": canonical_project_path,
        "stateId": state_id,
        "stateDigest": state_digest,
        "migrationReceiptIds": receipts,
        "links": links,
    }))
    .expect("migration risk input is serializable");
    RiskFingerprint::from(format!("risk:sha256:{:x}", Sha256::digest(bytes)))
}

fn managed_link_targets(
    project: &LegacyProjectSkillView,
) -> Result<Vec<PathBuf>, LegacySkillMigrationError> {
    let root = Path::new(
        project
            .canonical_project_path
            .as_deref()
            .ok_or(LegacySkillMigrationError::StateChanged)?,
    )
    .join(".claude/skills");
    project
        .links
        .iter()
        .map(|link| {
            let evidence = link
                .managed_evidence
                .as_ref()
                .ok_or(LegacySkillMigrationError::StateChanged)?;
            let target = root.join(&link.logical_id);
            if evidence.ownership_record.target_path != target.to_string_lossy() {
                return Err(LegacySkillMigrationError::StateChanged);
            }
            Ok(target)
        })
        .collect()
}

fn bind_managed_links(
    state: &ExecutionState,
    project: &LegacyProjectSkillView,
) -> Result<Vec<ConfinedTarget>, LegacySkillMigrationError> {
    let targets = managed_link_targets(project)?;
    project
        .links
        .iter()
        .zip(targets)
        .map(|(link, target)| {
            let evidence = link
                .managed_evidence
                .as_ref()
                .ok_or(LegacySkillMigrationError::StateChanged)?;
            let record = &evidence.ownership_record;
            let context = AgentContext {
                installation_id: record.resource.installation_id.clone(),
                project_path: record.resource.project_path.clone(),
            };
            let identity = capture_project_root_identity(&context)
                .map_err(|_| LegacySkillMigrationError::StateChanged)?;
            let confined = ConfinedTarget::resolve_bound(&record.resource, &target, identity)
                .map_err(|_| LegacySkillMigrationError::StateChanged)?;
            let observed = confined
                .observe()
                .map_err(|_| LegacySkillMigrationError::StateChanged)?;
            let TargetState::Symlink(target_path) = observed else {
                return Err(LegacySkillMigrationError::StateChanged);
            };
            let digest = ContentDigest::sha256(target_path.to_string_lossy().as_bytes());
            validate_ownership_record(
                record,
                &record.resource,
                &target,
                ResourceStateKind::Symlink,
                Some(&digest),
            )
            .and_then(|_| validate_ownership_artifact(record))
            .map_err(|_| LegacySkillMigrationError::StateChanged)?;
            if evidence.receipt_id != record.updated_by_receipt_id
                || !operation_receipt_proves_record(state, record, &digest)
            {
                return Err(LegacySkillMigrationError::StateChanged);
            }
            Ok(confined)
        })
        .collect()
}

fn archive_id(plan: &LegacySkillMigrationPlanView) -> String {
    archive_id_for_identity(
        &plan.canonical_project_path,
        &plan.state_id,
        &plan.state_digest,
    )
}

fn archive_id_for_identity(
    canonical_project_path: &str,
    state_id: &str,
    state_digest: &ContentDigest,
) -> String {
    let identity = format!(
        "{}\0{}\0{}",
        canonical_project_path,
        state_id,
        state_digest.as_str()
    );
    format!(
        "legacy-skill-archive:sha256:{:x}",
        Sha256::digest(identity.as_bytes())
    )
}

fn archive_name(archive_id: &str) -> String {
    format!("{}.legacy.json", opaque_name(archive_id))
}

fn marker_name(archive_id: &str) -> String {
    format!("{}.marker.json", opaque_name(archive_id))
}

fn marker(
    receipt: &LegacySkillMigrationReceipt,
    archive_name: &str,
    status: LegacySkillArchiveStatus,
) -> LegacySkillArchiveMarker {
    LegacySkillArchiveMarker {
        schema_version: MIGRATION_SCHEMA_VERSION,
        archive_id: receipt.archive_id.clone(),
        archive_name: archive_name.to_owned(),
        original_state_id: receipt.original_state_id.clone(),
        project_path: receipt.project_path.clone(),
        canonical_project_path: receipt.canonical_project_path.clone(),
        state_digest: receipt.state_digest.clone(),
        receipt_id: receipt
            .parent_receipt_id
            .clone()
            .unwrap_or_else(|| receipt.id.clone()),
        archived_at: receipt.created_at,
        status,
    }
}

fn restored_marker_matches_plan(
    state: &ExecutionState,
    plan: &LegacySkillMigrationPlanView,
    archive_name: &str,
) -> Result<bool, LegacySkillMigrationError> {
    let actual_marker = match read_marker(state, &archive_id(plan)) {
        Ok(marker) => marker,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error.into()),
    };
    let original = read_receipt(state, &actual_marker.receipt_id)
        .map_err(|_| LegacySkillMigrationError::StateChanged)?;
    let expected = marker(&original, archive_name, LegacySkillArchiveStatus::Restored);
    if actual_marker != expected
        || original.parent_receipt_id.is_some()
        || !matches!(
            original.status,
            LegacySkillMigrationReceiptStatus::Complete
                | LegacySkillMigrationReceiptStatus::Recovered
        )
        || original.archive_id != archive_id(plan)
        || original.original_state_id != plan.state_id
        || original.project_path != plan.project_path
        || original.canonical_project_path != plan.canonical_project_path
        || original.state_digest != plan.state_digest
    {
        return Err(LegacySkillMigrationError::StateChanged);
    }
    Ok(true)
}

fn validate_archived_marker(
    state: &ExecutionState,
    receipt: &LegacySkillMigrationReceipt,
    archive_name: &str,
) -> Result<(), LegacySkillMigrationError> {
    let expected_archive_id = archive_id_for_identity(
        &receipt.canonical_project_path,
        &receipt.original_state_id,
        &receipt.state_digest,
    );
    if receipt.archive_id != expected_archive_id {
        return Err(LegacySkillMigrationError::RollbackUnavailable);
    }
    let actual = read_marker(state, &receipt.archive_id)
        .map_err(|_| LegacySkillMigrationError::RollbackUnavailable)?;
    let expected = marker(receipt, archive_name, LegacySkillArchiveStatus::Archived);
    if actual != expected {
        return Err(LegacySkillMigrationError::RollbackUnavailable);
    }
    Ok(())
}

fn read_marker(
    state: &ExecutionState,
    archive_id: &str,
) -> Result<LegacySkillArchiveMarker, std::io::Error> {
    let bytes = state
        .skill_migration_archive()
        .read(&marker_name(archive_id))?;
    let marker: LegacySkillArchiveMarker =
        serde_json::from_slice(&bytes).map_err(std::io::Error::other)?;
    if marker.schema_version != MIGRATION_SCHEMA_VERSION {
        return Err(std::io::Error::other(
            "unsupported legacy Skill archive marker version",
        ));
    }
    Ok(marker)
}

fn finalize_archive(
    state: &ExecutionState,
    journal: &MigrationJournal,
    replace_marker: bool,
) -> Result<(), std::io::Error> {
    let archive_name = archive_name(&journal.receipt.archive_id);
    let bytes = state.skill_migration_archive().read(&archive_name)?;
    if ContentDigest::sha256(&bytes) != journal.receipt.state_digest {
        return Err(std::io::Error::other(
            "archived legacy state digest changed",
        ));
    }
    persist_marker(state, &journal.marker, replace_marker)?;
    persist_receipt(state, &journal.receipt)
}

fn digest_matches(
    directory: &super::execution_state::StateDirectory,
    name: &str,
    expected: &ContentDigest,
) -> Result<bool, std::io::Error> {
    Ok(ContentDigest::sha256(&directory.read(name)?) == *expected)
}

fn entry_exists(
    directory: &super::execution_state::StateDirectory,
    name: &str,
) -> Result<bool, std::io::Error> {
    match directory.read(name) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

fn compensate_recovery(
    state: &ExecutionState,
    journal_name: &str,
    journal: &mut MigrationJournal,
    report: &mut LegacySkillMigrationRecoveryReport,
) -> Result<(), std::io::Error> {
    journal.receipt.status = LegacySkillMigrationReceiptStatus::Compensated;
    persist_receipt(state, &journal.receipt)?;
    journal.state = MigrationJournalState::Compensated;
    persist_journal(state, journal_name, journal, true)?;
    report.compensated += 1;
    Ok(())
}

fn finalize_rollback(
    state: &ExecutionState,
    journal: &MigrationJournal,
) -> Result<(), std::io::Error> {
    let bytes = state
        .legacy_project_skills()
        .read(&journal.receipt.original_state_id)?;
    if ContentDigest::sha256(&bytes) != journal.receipt.state_digest {
        return Err(std::io::Error::other(
            "restored legacy state digest changed",
        ));
    }
    persist_marker(state, &journal.marker, true)?;
    persist_receipt(state, &journal.receipt)
}

fn persist_marker(
    state: &ExecutionState,
    marker: &LegacySkillArchiveMarker,
    replace: bool,
) -> Result<(), std::io::Error> {
    let name = marker_name(&marker.archive_id);
    let bytes = serde_json::to_vec_pretty(marker).map_err(std::io::Error::other)?;
    if replace {
        state.skill_migration_archive().write_atomic(&name, &bytes)
    } else {
        write_new_idempotent(state.skill_migration_archive(), &name, &bytes)
    }
}

fn persist_receipt(
    state: &ExecutionState,
    receipt: &LegacySkillMigrationReceipt,
) -> Result<(), std::io::Error> {
    let name = receipt_name(&receipt.id);
    let bytes = serde_json::to_vec_pretty(receipt).map_err(std::io::Error::other)?;
    write_new_idempotent(state.skill_migration_history(), &name, &bytes)
}

fn migration_receipt_persisted(
    state: &ExecutionState,
    receipt: &LegacySkillMigrationReceipt,
) -> Result<bool, std::io::Error> {
    let bytes = match state
        .skill_migration_history()
        .read(&receipt_name(&receipt.id))
    {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
    };
    let existing: LegacySkillMigrationReceipt =
        serde_json::from_slice(&bytes).map_err(std::io::Error::other)?;
    if existing != *receipt {
        return Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "Skill migration receipt identity collision",
        ));
    }
    Ok(true)
}

fn read_receipt(
    state: &ExecutionState,
    receipt_id: &ReceiptId,
) -> Result<LegacySkillMigrationReceipt, LegacySkillMigrationError> {
    let bytes = state
        .skill_migration_history()
        .read(&receipt_name(receipt_id))
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                LegacySkillMigrationError::ReceiptNotFound
            } else {
                LegacySkillMigrationError::Io(error)
            }
        })?;
    let receipt: LegacySkillMigrationReceipt =
        serde_json::from_slice(&bytes).map_err(|_| LegacySkillMigrationError::ReceiptNotFound)?;
    if receipt.schema_version != MIGRATION_SCHEMA_VERSION || &receipt.id != receipt_id {
        return Err(LegacySkillMigrationError::ReceiptNotFound);
    }
    Ok(receipt)
}

fn persist_journal(
    state: &ExecutionState,
    name: &str,
    journal: &MigrationJournal,
    replace: bool,
) -> Result<(), std::io::Error> {
    let bytes = serde_json::to_vec_pretty(journal).map_err(std::io::Error::other)?;
    if replace {
        state.skill_migration_journals().write_atomic(name, &bytes)
    } else {
        state
            .skill_migration_journals()
            .write_atomic_new(name, &bytes)
    }
}

fn write_new_idempotent(
    directory: &super::execution_state::StateDirectory,
    name: &str,
    bytes: &[u8],
) -> Result<(), std::io::Error> {
    match directory.write_atomic_new(name, bytes) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            if directory.read(name)? == bytes {
                Ok(())
            } else {
                Err(std::io::Error::new(
                    std::io::ErrorKind::AlreadyExists,
                    "Skill migration record identity collision",
                ))
            }
        }
        Err(error) => Err(error),
    }
}

fn ensure_recovery_writable(state: &ExecutionState) -> Result<(), LegacySkillMigrationError> {
    for name in state.skill_migration_journals().entry_names()? {
        let Some(name) = name.to_str() else {
            return Err(LegacySkillMigrationError::RepairRequired);
        };
        let bytes = state.skill_migration_journals().read(name)?;
        let journal: MigrationJournal = serde_json::from_slice(&bytes)
            .map_err(|_| LegacySkillMigrationError::RepairRequired)?;
        if journal.schema_version != MIGRATION_SCHEMA_VERSION
            || matches!(
                journal.state,
                MigrationJournalState::Prepared
                    | MigrationJournalState::Applying
                    | MigrationJournalState::RepairRequired
            )
        {
            return Err(LegacySkillMigrationError::RepairRequired);
        }
    }
    Ok(())
}

fn journal_name(plan_id: &PlanId) -> String {
    format!("{}.json", opaque_name(plan_id.as_str()))
}

fn receipt_name(receipt_id: &ReceiptId) -> String {
    format!("{}.json", opaque_name(receipt_id.as_str()))
}

fn opaque_name(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}

fn lock_error(error: std::io::Error) -> LegacySkillMigrationError {
    if error.kind() == std::io::ErrorKind::WouldBlock {
        LegacySkillMigrationError::Locked
    } else {
        LegacySkillMigrationError::Io(error)
    }
}

fn mark_repair(report: &mut LegacySkillMigrationRecoveryReport, message: impl Into<String>) {
    report.repair_required += 1;
    report.diagnostics.push(message.into());
}

#[cfg(test)]
mod tests {
    use serial_test::serial;

    use super::*;

    fn fixture(
        state: &ExecutionState,
        operation: MigrationOperation,
    ) -> (MigrationJournal, String, Vec<u8>) {
        let bytes =
            br#"{"projectPath":"/tmp/project","listedSkills":[],"mode":"allowlist"}"#.to_vec();
        let plan = LegacySkillMigrationPlanView {
            schema_version: MIGRATION_SCHEMA_VERSION,
            id: PlanId::from(format!("plan:{}", uuid::Uuid::new_v4())),
            project_path: "/tmp/project".into(),
            canonical_project_path: "/tmp/project".into(),
            state_id: "tmp-project.json".into(),
            state_digest: ContentDigest::sha256(&bytes),
            migration_receipt_ids: Vec::new(),
            confirmation_required: true,
            risk_fingerprint: RiskFingerprint::from("risk:test"),
            expires_at: Utc::now() + Duration::minutes(5),
        };
        let archive_id = archive_id(&plan);
        let archive_name = archive_name(&archive_id);
        let receipt = LegacySkillMigrationReceipt {
            schema_version: MIGRATION_SCHEMA_VERSION,
            id: ReceiptId::from(format!("receipt:{}", uuid::Uuid::new_v4())),
            plan_id: plan.id.clone(),
            parent_receipt_id: (operation == MigrationOperation::Rollback)
                .then(|| ReceiptId::from("receipt:archive")),
            archive_id,
            original_state_id: plan.state_id,
            project_path: plan.project_path,
            canonical_project_path: plan.canonical_project_path,
            state_digest: plan.state_digest,
            migration_receipt_ids: Vec::new(),
            status: LegacySkillMigrationReceiptStatus::Complete,
            created_at: Utc::now(),
        };
        let marker = marker(
            &receipt,
            &archive_name,
            if operation == MigrationOperation::Archive {
                LegacySkillArchiveStatus::Archived
            } else {
                LegacySkillArchiveStatus::Restored
            },
        );
        let journal = MigrationJournal {
            schema_version: MIGRATION_SCHEMA_VERSION,
            instance_id: execution_instance_id().into(),
            operation_id: uuid::Uuid::new_v4().to_string(),
            operation,
            state: MigrationJournalState::Applying,
            receipt,
            marker,
        };
        let name = journal_name(&journal.receipt.plan_id);
        persist_journal(state, &name, &journal, false).unwrap();
        (journal, name, bytes)
    }

    #[test]
    #[serial(home_env)]
    fn recovery_finishes_archive_after_move_before_marker_persistence() {
        let home = tempfile::tempdir().unwrap();
        std::env::set_var("AD_HOME", home.path());
        let state = ExecutionState::open().unwrap();
        let (journal, _, bytes) = fixture(&state, MigrationOperation::Archive);
        state
            .legacy_project_skills()
            .write_atomic(&journal.receipt.original_state_id, &bytes)
            .unwrap();
        let archive_name = archive_name(&journal.receipt.archive_id);
        state
            .legacy_project_skills()
            .rename_entry_to(
                &journal.receipt.original_state_id,
                state.skill_migration_archive(),
                &archive_name,
            )
            .unwrap();
        let report = recover_legacy_skill_migrations().unwrap();

        assert_eq!(report.recovered, 1);
        assert!(report.writable());
        let receipt = read_receipt(&state, &journal.receipt.id).unwrap();
        assert_eq!(receipt.status, LegacySkillMigrationReceiptStatus::Recovered);
        assert_eq!(
            state.skill_migration_archive().read(&archive_name).unwrap(),
            bytes
        );
        assert!(read_marker(&state, &journal.receipt.archive_id).is_ok());
    }

    #[test]
    #[serial(home_env)]
    fn recovery_accepts_receipt_persisted_before_terminal_journal() {
        let home = tempfile::tempdir().unwrap();
        std::env::set_var("AD_HOME", home.path());
        let state = ExecutionState::open().unwrap();
        let (journal, _, bytes) = fixture(&state, MigrationOperation::Archive);
        state
            .legacy_project_skills()
            .write_atomic(&journal.receipt.original_state_id, &bytes)
            .unwrap();
        let archive_name = archive_name(&journal.receipt.archive_id);
        state
            .legacy_project_skills()
            .rename_entry_to(
                &journal.receipt.original_state_id,
                state.skill_migration_archive(),
                &archive_name,
            )
            .unwrap();
        persist_marker(&state, &journal.marker, false).unwrap();
        persist_receipt(&state, &journal.receipt).unwrap();

        let report = recover_legacy_skill_migrations().unwrap();

        assert_eq!(report.recovered, 1);
        assert!(report.writable());
        assert_eq!(
            read_receipt(&state, &journal.receipt.id).unwrap().status,
            LegacySkillMigrationReceiptStatus::Complete
        );
    }

    #[test]
    #[serial(home_env)]
    fn recovery_compensates_when_cleanup_never_moved_the_source() {
        let home = tempfile::tempdir().unwrap();
        std::env::set_var("AD_HOME", home.path());
        let state = ExecutionState::open().unwrap();
        let (journal, _, bytes) = fixture(&state, MigrationOperation::Archive);
        state
            .legacy_project_skills()
            .write_atomic(&journal.receipt.original_state_id, &bytes)
            .unwrap();

        let report = recover_legacy_skill_migrations().unwrap();

        assert_eq!(report.compensated, 1);
        assert!(report.writable());
        assert_eq!(
            state
                .legacy_project_skills()
                .read(&journal.receipt.original_state_id)
                .unwrap(),
            bytes
        );
        assert_eq!(
            read_receipt(&state, &journal.receipt.id).unwrap().status,
            LegacySkillMigrationReceiptStatus::Compensated
        );
    }

    #[test]
    #[serial(home_env)]
    fn recovery_restores_bytes_changed_during_the_archive_race() {
        let home = tempfile::tempdir().unwrap();
        std::env::set_var("AD_HOME", home.path());
        let state = ExecutionState::open().unwrap();
        let (journal, _, _) = fixture(&state, MigrationOperation::Archive);
        let changed = br#"{"projectPath":"/tmp/project","listedSkills":["changed/review"],"mode":"allowlist"}"#;
        let archive_name = archive_name(&journal.receipt.archive_id);
        state
            .skill_migration_archive()
            .write_atomic(&archive_name, changed)
            .unwrap();

        let report = recover_legacy_skill_migrations().unwrap();

        assert_eq!(report.compensated, 1);
        assert_eq!(
            state
                .legacy_project_skills()
                .read(&journal.receipt.original_state_id)
                .unwrap(),
            changed
        );
        assert!(state.skill_migration_archive().read(&archive_name).is_err());
    }

    #[test]
    #[serial(home_env)]
    fn recovery_finishes_interrupted_restore_without_touching_project_links() {
        let home = tempfile::tempdir().unwrap();
        std::env::set_var("AD_HOME", home.path());
        let state = ExecutionState::open().unwrap();
        let (journal, _, bytes) = fixture(&state, MigrationOperation::Rollback);
        let archive_name = archive_name(&journal.receipt.archive_id);
        state
            .skill_migration_archive()
            .write_atomic(&archive_name, &bytes)
            .unwrap();
        state
            .skill_migration_archive()
            .rename_entry_to(
                &archive_name,
                state.legacy_project_skills(),
                &journal.receipt.original_state_id,
            )
            .unwrap();

        let report = recover_legacy_skill_migrations().unwrap();

        assert_eq!(report.recovered, 1);
        assert_eq!(
            read_receipt(&state, &journal.receipt.id).unwrap().status,
            LegacySkillMigrationReceiptStatus::Restored
        );
        assert_eq!(
            state
                .legacy_project_skills()
                .read(&journal.receipt.original_state_id)
                .unwrap(),
            bytes
        );
    }
}
