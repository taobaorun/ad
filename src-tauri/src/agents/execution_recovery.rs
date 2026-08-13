use std::fs::File;
use std::os::unix::ffi::OsStrExt;

use rustix::fs::{flock, FlockOperation};

use super::execution_journal::{
    OperationJournal, OperationJournalHandle, OperationJournalState, JOURNAL_SCHEMA_VERSION,
    MIN_JOURNAL_SCHEMA_VERSION,
};
use super::execution_state::ExecutionState;
use super::{
    apply_ownership_changes, decode_operation_receipt, reconcile_installations, AgentError,
    AgentErrorCode, OperationStatus, ReceiptId,
};

const RECOVERY_LOCK_NAME: &str = "recovery.lock";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RecoveryReport {
    pub inspected: usize,
    pub recovered: usize,
    pub repair_required: usize,
    pub diagnostics: Vec<String>,
}

impl RecoveryReport {
    pub fn writable(&self) -> bool {
        self.repair_required == 0 && self.diagnostics.is_empty()
    }
}

pub fn recover_execution_state() -> Result<RecoveryReport, AgentError> {
    let state = ExecutionState::open().map_err(recovery_error)?;
    recover_state(&state)
}

#[derive(Debug)]
pub(super) struct MutationRecoveryLease {
    _file: File,
}

impl MutationRecoveryLease {
    pub(super) fn acquire(state: &ExecutionState) -> Result<Self, AgentError> {
        let file = state
            .locks()
            .open_lock(RECOVERY_LOCK_NAME)
            .map_err(recovery_error)?;
        lock(&file, FlockOperation::NonBlockingLockShared)?;
        ensure_mutation_allowed(state, None)?;
        Ok(Self { _file: file })
    }

    pub(super) fn acquire_for_rollback(
        state: &ExecutionState,
        receipt_id: &ReceiptId,
    ) -> Result<Self, AgentError> {
        let file = state
            .locks()
            .open_lock(RECOVERY_LOCK_NAME)
            .map_err(recovery_error)?;
        lock(&file, FlockOperation::NonBlockingLockShared)?;
        ensure_mutation_allowed(state, Some(receipt_id))?;
        Ok(Self { _file: file })
    }
}

pub(super) fn mark_repaired(
    state: &ExecutionState,
    receipt_id: &ReceiptId,
) -> Result<(), AgentError> {
    for name in journal_names(state)? {
        let journal = read_journal(state, &name).map_err(|message| AgentError {
            code: AgentErrorCode::PartialFailure,
            message: format!("Failed to finalize repaired operation journal: {message}"),
            agent_id: None,
            installation_id: None,
            resource: None,
            retryable: false,
            details: Some(serde_json::json!({"phase": "operation_recovery"})),
        })?;
        if journal.state == OperationJournalState::RepairRequired
            && journal.receipt_id.as_ref() == Some(receipt_id)
        {
            OperationJournalHandle::load(state, name, journal)
                .and_then(|mut handle| {
                    handle.transition(OperationJournalState::Repaired, Some(receipt_id))
                })
                .map_err(recovery_error)?;
        }
    }
    Ok(())
}

pub(super) fn recover_state(state: &ExecutionState) -> Result<RecoveryReport, AgentError> {
    let file = state
        .locks()
        .open_lock(RECOVERY_LOCK_NAME)
        .map_err(recovery_error)?;
    lock(&file, FlockOperation::NonBlockingLockExclusive)?;
    let mut report = RecoveryReport::default();
    for name in journal_names(state)? {
        report.inspected += 1;
        let journal = match read_journal(state, &name) {
            Ok(journal) => journal,
            Err(message) => {
                report.diagnostics.push(format!("{name}: {message}"));
                continue;
            }
        };
        match recover_journal(state, &name, journal) {
            Ok(RecoveryOutcome::Terminal) => {}
            Ok(RecoveryOutcome::Recovered) => report.recovered += 1,
            Ok(RecoveryOutcome::RepairRequired(message)) => {
                report.repair_required += 1;
                report.diagnostics.push(format!("{name}: {message}"));
            }
            Err(error) => {
                report.repair_required += 1;
                report.diagnostics.push(format!("{name}: {error}"));
            }
        }
    }
    Ok(report)
}

fn recover_journal(
    state: &ExecutionState,
    name: &str,
    journal: OperationJournal,
) -> Result<RecoveryOutcome, std::io::Error> {
    match journal.state {
        OperationJournalState::Committed
        | OperationJournalState::Compensated
        | OperationJournalState::Repaired => Ok(RecoveryOutcome::Terminal),
        OperationJournalState::RepairRequired => Ok(RecoveryOutcome::RepairRequired(
            "operation already requires repair".into(),
        )),
        OperationJournalState::Prepared => {
            let mut handle = OperationJournalHandle::load(state, name.to_owned(), journal.clone())?;
            match state.backups().remove(journal.planned_receipt_id.as_str()) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    handle.transition(OperationJournalState::RepairRequired, None)?;
                    return Ok(RecoveryOutcome::RepairRequired(format!(
                        "failed to clean prepared backups: {error}"
                    )));
                }
            }
            handle.transition(OperationJournalState::Compensated, None)?;
            Ok(RecoveryOutcome::Recovered)
        }
        OperationJournalState::Applying => recover_applying(state, name, journal),
    }
}

fn recover_applying(
    state: &ExecutionState,
    name: &str,
    journal: OperationJournal,
) -> Result<RecoveryOutcome, std::io::Error> {
    let receipt_name = format!("{}.json", journal.planned_receipt_id);
    let receipt = match state.history().read(&receipt_name) {
        Ok(bytes) => decode_operation_receipt(&bytes).ok(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error),
    };
    let mut handle = OperationJournalHandle::load(state, name.to_owned(), journal.clone())?;
    let Some(receipt) = receipt else {
        handle.transition(OperationJournalState::RepairRequired, None)?;
        return Ok(RecoveryOutcome::RepairRequired(
            "applying operation has no valid receipt".into(),
        ));
    };
    if receipt.id != journal.planned_receipt_id || receipt.plan_id != journal.plan_id {
        handle.transition(OperationJournalState::RepairRequired, None)?;
        return Ok(RecoveryOutcome::RepairRequired(
            "receipt identity does not match the journal".into(),
        ));
    }
    if receipt.status == OperationStatus::Complete {
        if let Err(error) = apply_ownership_changes(state, &receipt.ownership_changes) {
            handle.transition(OperationJournalState::RepairRequired, Some(&receipt.id))?;
            return Ok(RecoveryOutcome::RepairRequired(format!(
                "ownership reconciliation failed: {error}"
            )));
        }
        if let Err(error) = reconcile_installations(state, &receipt.ownership_changes) {
            handle.transition(OperationJournalState::RepairRequired, Some(&receipt.id))?;
            return Ok(RecoveryOutcome::RepairRequired(format!(
                "installation reconciliation failed: {error}"
            )));
        }
    }
    let recovered_state = match receipt.status {
        OperationStatus::Complete => OperationJournalState::Committed,
        OperationStatus::Compensated => OperationJournalState::Compensated,
        OperationStatus::PartialFailure => OperationJournalState::RepairRequired,
    };
    handle.transition(recovered_state, Some(&receipt.id))?;
    if recovered_state == OperationJournalState::RepairRequired {
        Ok(RecoveryOutcome::RepairRequired(
            "partial receipt requires repair".into(),
        ))
    } else {
        Ok(RecoveryOutcome::Recovered)
    }
}

fn ensure_mutation_allowed(
    state: &ExecutionState,
    allowed_repair: Option<&ReceiptId>,
) -> Result<(), AgentError> {
    let mut blocked = Vec::new();
    for name in journal_names(state)? {
        match read_journal(state, &name) {
            Ok(journal)
                if matches!(
                    journal.state,
                    OperationJournalState::Committed
                        | OperationJournalState::Compensated
                        | OperationJournalState::Repaired
                ) => {}
            Ok(journal)
                if journal.state == OperationJournalState::RepairRequired
                    && allowed_repair.is_some()
                    && journal.receipt_id.as_ref() == allowed_repair => {}
            Ok(journal) => blocked.push(format!("{name}:{:?}", journal.state)),
            Err(message) => blocked.push(format!("{name}:{message}")),
        }
    }
    if blocked.is_empty() {
        Ok(())
    } else {
        Err(AgentError {
            code: AgentErrorCode::PartialFailure,
            message: "Agent mutations are blocked until operation recovery is resolved".into(),
            agent_id: None,
            installation_id: None,
            resource: None,
            retryable: false,
            details: Some(serde_json::json!({
                "phase": "operation_recovery",
                "blockedJournals": blocked,
            })),
        })
    }
}

fn journal_names(state: &ExecutionState) -> Result<Vec<String>, AgentError> {
    let mut names = Vec::new();
    for name in state.journals().entry_names().map_err(recovery_error)? {
        if !name.as_bytes().ends_with(b".json") {
            continue;
        }
        names.push(name.into_string().map_err(|_| {
            recovery_error(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Operation journal name is not valid UTF-8",
            ))
        })?);
    }
    Ok(names)
}

fn read_journal(state: &ExecutionState, name: &str) -> Result<OperationJournal, String> {
    let bytes = state
        .journals()
        .read(name)
        .map_err(|error| error.to_string())?;
    let value: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|error| format!("invalid JSON: {error}"))?;
    let version = value
        .get("schemaVersion")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| "missing journal schema version".to_owned())?;
    if !(u64::from(MIN_JOURNAL_SCHEMA_VERSION)..=u64::from(JOURNAL_SCHEMA_VERSION))
        .contains(&version)
    {
        return Err(format!("unsupported journal schema version {version}"));
    }
    serde_json::from_value(value).map_err(|error| format!("invalid journal: {error}"))
}

fn lock(file: &File, operation: FlockOperation) -> Result<(), AgentError> {
    flock(file, operation).map_err(|error| AgentError {
        code: if error == rustix::io::Errno::WOULDBLOCK {
            AgentErrorCode::ResourceChanged
        } else {
            AgentErrorCode::Io
        },
        message: format!("Failed to acquire the operation recovery lock: {error}"),
        agent_id: None,
        installation_id: None,
        resource: None,
        retryable: error == rustix::io::Errno::WOULDBLOCK,
        details: Some(serde_json::json!({"phase": "operation_recovery"})),
    })
}

fn recovery_error(error: std::io::Error) -> AgentError {
    AgentError {
        code: AgentErrorCode::Io,
        message: format!("Failed to inspect operation recovery state: {error}"),
        agent_id: None,
        installation_id: None,
        resource: None,
        retryable: false,
        details: Some(serde_json::json!({"phase": "operation_recovery"})),
    }
}

enum RecoveryOutcome {
    Terminal,
    Recovered,
    RepairRequired(String),
}
