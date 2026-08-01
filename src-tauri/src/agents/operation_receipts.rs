use std::collections::BTreeSet;
use std::fmt;
use std::os::unix::ffi::OsStrExt;

use chrono::{DateTime, Utc};

use super::execution_state::ExecutionState;
use super::{
    AgentError, AgentErrorCode, InstallationId, OperationHistoryDiagnostic,
    OperationHistoryDiagnosticCode, OperationHistoryEntry, OperationReceipt, RollbackEligibility,
    RollbackUnavailableReason, OPERATION_RECEIPT_SCHEMA_VERSION,
};

const LEGACY_OPERATION_RECEIPT_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OperationReceiptDecodeError {
    Malformed(String),
    UnsupportedVersion(u64),
}

impl OperationReceiptDecodeError {
    fn diagnostic_code(&self) -> OperationHistoryDiagnosticCode {
        match self {
            Self::Malformed(_) => OperationHistoryDiagnosticCode::Malformed,
            Self::UnsupportedVersion(_) => OperationHistoryDiagnosticCode::UnsupportedVersion,
        }
    }
}

impl fmt::Display for OperationReceiptDecodeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Malformed(message) => formatter.write_str(message),
            Self::UnsupportedVersion(version) => {
                write!(
                    formatter,
                    "Unsupported operation receipt schema version {version}"
                )
            }
        }
    }
}

pub fn decode_operation_receipt(
    bytes: &[u8],
) -> Result<OperationReceipt, OperationReceiptDecodeError> {
    let value: serde_json::Value = serde_json::from_slice(bytes)
        .map_err(|error| OperationReceiptDecodeError::Malformed(error.to_string()))?;
    let version = match value.get("schemaVersion") {
        Some(version) => version.as_u64().ok_or_else(|| {
            OperationReceiptDecodeError::Malformed(
                "Operation receipt schema version must be an unsigned integer".into(),
            )
        })?,
        None => u64::from(LEGACY_OPERATION_RECEIPT_SCHEMA_VERSION),
    };
    match version {
        version if version == u64::from(LEGACY_OPERATION_RECEIPT_SCHEMA_VERSION) => {
            let mut receipt: OperationReceipt = serde_json::from_value(value)
                .map_err(|error| OperationReceiptDecodeError::Malformed(error.to_string()))?;
            receipt.schema_version = LEGACY_OPERATION_RECEIPT_SCHEMA_VERSION;
            receipt.rollback =
                RollbackEligibility::unavailable(RollbackUnavailableReason::LegacyReceipt);
            Ok(receipt)
        }
        version if version == u64::from(OPERATION_RECEIPT_SCHEMA_VERSION) => {
            for field in ["operationKind", "context", "rollback", "createdAt"] {
                if value.get(field).is_none() {
                    return Err(OperationReceiptDecodeError::Malformed(format!(
                        "Version {version} operation receipt is missing {field}"
                    )));
                }
            }
            serde_json::from_value(value)
                .map_err(|error| OperationReceiptDecodeError::Malformed(error.to_string()))
        }
        version => Err(OperationReceiptDecodeError::UnsupportedVersion(version)),
    }
}

pub fn list_operation_history(
    installation_id: Option<&InstallationId>,
    project_path: Option<&str>,
    limit: Option<usize>,
) -> Result<Vec<OperationHistoryEntry>, AgentError> {
    let state = ExecutionState::open().map_err(operation_history_io_error)?;
    let mut history = Vec::new();
    for name in state
        .history()
        .entry_names()
        .map_err(operation_history_io_error)?
    {
        if !name.as_bytes().ends_with(b".json") {
            continue;
        }
        let source = name.to_string_lossy().into_owned();
        let created_at = name
            .to_str()
            .and_then(|name| state.history().modified(name).ok())
            .map(DateTime::<Utc>::from)
            .unwrap_or_else(Utc::now);
        let Some(name) = name.to_str() else {
            history.push(diagnostic_entry(
                OperationHistoryDiagnosticCode::Unreadable,
                source,
                "Operation receipt name is not valid UTF-8".into(),
                created_at,
            ));
            continue;
        };
        let bytes = match state.history().read(name) {
            Ok(bytes) => bytes,
            Err(error) => {
                history.push(diagnostic_entry(
                    OperationHistoryDiagnosticCode::Unreadable,
                    source,
                    error.to_string(),
                    created_at,
                ));
                continue;
            }
        };
        let receipt = match decode_operation_receipt(&bytes) {
            Ok(receipt) => receipt,
            Err(error) => {
                history.push(diagnostic_entry(
                    error.diagnostic_code(),
                    source,
                    error.to_string(),
                    created_at,
                ));
                continue;
            }
        };
        if name.strip_suffix(".json") != Some(receipt.id.as_str()) {
            history.push(diagnostic_entry(
                OperationHistoryDiagnosticCode::Malformed,
                source,
                "Operation receipt id does not match its file identity".into(),
                created_at,
            ));
            continue;
        }
        if !receipt_matches_filter(&receipt, installation_id, project_path) {
            continue;
        }
        let created_at = receipt.created_at.unwrap_or(created_at);
        history.push(OperationHistoryEntry {
            receipt: Some(receipt),
            diagnostic: None,
            created_at,
        });
    }
    let rolled_back = history
        .iter()
        .filter_map(|entry| entry.receipt.as_ref())
        .filter(|receipt| {
            receipt.operation_kind == super::OperationKind::Rollback
                && receipt.status == super::OperationStatus::Complete
        })
        .filter_map(|receipt| receipt.parent_receipt_id.clone())
        .collect::<BTreeSet<_>>();
    for receipt in history
        .iter_mut()
        .filter_map(|entry| entry.receipt.as_mut())
    {
        if rolled_back.contains(&receipt.id) {
            receipt.rollback =
                RollbackEligibility::unavailable(RollbackUnavailableReason::AlreadyRolledBack);
        }
        if project_path.is_some()
            && receipt
                .context
                .as_ref()
                .and_then(|context| context.project_path.as_deref())
                != project_path
        {
            receipt.rollback =
                RollbackEligibility::unavailable(RollbackUnavailableReason::WorkspaceMismatch);
        }
    }
    history.sort_by_key(|entry| std::cmp::Reverse(entry.created_at));
    history.truncate(limit.unwrap_or(50).min(200));
    Ok(history)
}

fn receipt_matches_filter(
    receipt: &OperationReceipt,
    installation_id: Option<&InstallationId>,
    project_path: Option<&str>,
) -> bool {
    if installation_id.is_none() && project_path.is_none() {
        return true;
    }
    receipt
        .applied_resources
        .iter()
        .chain(
            receipt
                .post_apply_states
                .iter()
                .map(|state| &state.resource),
        )
        .any(|resource| {
            let installation_matches =
                installation_id.is_some_and(|expected| &resource.installation_id == expected);
            let project_matches = project_path
                .is_some_and(|expected| resource.project_path.as_deref() == Some(expected));
            match (installation_id, project_path) {
                (Some(_), Some(_)) => {
                    project_matches || (installation_matches && resource.project_path.is_none())
                }
                (Some(_), None) => installation_matches,
                (None, Some(_)) => project_matches,
                (None, None) => true,
            }
        })
}

fn diagnostic_entry(
    code: OperationHistoryDiagnosticCode,
    source: String,
    message: String,
    created_at: DateTime<Utc>,
) -> OperationHistoryEntry {
    OperationHistoryEntry {
        receipt: None,
        diagnostic: Some(OperationHistoryDiagnostic {
            code,
            source,
            message,
        }),
        created_at,
    }
}

fn operation_history_io_error(error: std::io::Error) -> AgentError {
    AgentError {
        code: AgentErrorCode::Io,
        message: format!("Failed to inspect operation history: {error}"),
        agent_id: None,
        installation_id: None,
        resource: None,
        retryable: false,
        details: Some(serde_json::json!({"phase": "operation_history"})),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::{
        AgentContext, ContentDigest, OperationKind, OperationStatus, PlanId, ReceiptId,
        ResourceKind, ResourceRef, ResourceScope, OWNERSHIP_EVIDENCE_VERSION,
    };

    #[test]
    fn legacy_receipts_remain_visible_but_cannot_rollback() {
        let receipt = decode_operation_receipt(
            br#"{
                "id":"legacy-receipt",
                "planId":"legacy-plan",
                "status":"complete",
                "appliedResources":[],
                "backupPaths":[],
                "postApplyStates":[]
            }"#,
        )
        .unwrap();

        assert_eq!(receipt.schema_version, 1);
        assert!(!receipt.rollback.available);
        assert_eq!(
            receipt.rollback.reason,
            Some(RollbackUnavailableReason::LegacyReceipt)
        );
    }

    #[test]
    fn current_receipts_require_current_evidence_fields() {
        let receipt = OperationReceipt {
            schema_version: OPERATION_RECEIPT_SCHEMA_VERSION,
            id: ReceiptId::from("current-receipt"),
            plan_id: PlanId::from("current-plan"),
            operation_kind: OperationKind::Apply,
            parent_receipt_id: None,
            context: Some(AgentContext {
                installation_id: InstallationId::from("codex:default"),
                project_path: Some("/Users/test/project".into()),
            }),
            workspace_key: None,
            action_id: None,
            status: OperationStatus::Complete,
            applied_resources: Vec::new(),
            backup_paths: Vec::new(),
            post_apply_states: Vec::new(),
            manifest_digest: None,
            ownership_changes: Vec::new(),
            ownership_evidence_version: 0,
            rollback: RollbackEligibility::available(),
            created_at: Some(Utc::now()),
            message: None,
        };
        let bytes = serde_json::to_vec(&receipt).unwrap();

        assert_eq!(decode_operation_receipt(&bytes).unwrap(), receipt);

        let mut missing_context = serde_json::to_value(receipt).unwrap();
        missing_context.as_object_mut().unwrap().remove("context");
        let error =
            decode_operation_receipt(&serde_json::to_vec(&missing_context).unwrap()).unwrap_err();
        assert!(error.to_string().contains("context"));
    }

    #[test]
    fn future_receipts_are_isolated_by_the_decoder() {
        let error = decode_operation_receipt(br#"{"schemaVersion":999}"#).unwrap_err();

        assert_eq!(error, OperationReceiptDecodeError::UnsupportedVersion(999));
    }

    #[test]
    #[serial_test::serial(home_env)]
    fn history_keeps_valid_legacy_receipts_beside_corrupt_and_future_diagnostics() {
        let temp = tempfile::tempdir().unwrap();
        let previous_home = std::env::var("AD_HOME").ok();
        std::env::set_var("AD_HOME", temp.path());
        let state = ExecutionState::open().unwrap();
        state
            .history()
            .write_atomic_new(
                "legacy-receipt.json",
                br#"{
                    "id":"legacy-receipt",
                    "planId":"legacy-plan",
                    "status":"complete",
                    "appliedResources":[],
                    "backupPaths":[],
                    "postApplyStates":[]
                }"#,
            )
            .unwrap();
        state
            .history()
            .write_atomic_new("corrupt.json", b"not-json")
            .unwrap();
        state
            .history()
            .write_atomic_new("future.json", br#"{"schemaVersion":999}"#)
            .unwrap();
        state
            .history()
            .write_atomic_new(
                "wrong-name.json",
                br#"{
                    "id":"different-id",
                    "planId":"legacy-plan",
                    "status":"complete",
                    "appliedResources":[],
                    "backupPaths":[],
                    "postApplyStates":[]
                }"#,
            )
            .unwrap();

        let history = list_operation_history(None, None, Some(20)).unwrap();

        assert_eq!(history.len(), 4);
        assert!(history.iter().any(|entry| {
            entry
                .receipt
                .as_ref()
                .is_some_and(|receipt| receipt.id.as_str() == "legacy-receipt")
        }));
        assert!(!history.iter().any(|entry| {
            entry
                .receipt
                .as_ref()
                .is_some_and(|receipt| receipt.id.as_str() == "different-id")
        }));
        assert!(history.iter().any(|entry| {
            entry.diagnostic.as_ref().is_some_and(|diagnostic| {
                diagnostic.code == OperationHistoryDiagnosticCode::Malformed
            })
        }));
        assert!(history.iter().any(|entry| {
            entry.diagnostic.as_ref().is_some_and(|diagnostic| {
                diagnostic.code == OperationHistoryDiagnosticCode::UnsupportedVersion
            })
        }));

        match previous_home {
            Some(value) => std::env::set_var("AD_HOME", value),
            None => std::env::remove_var("AD_HOME"),
        }
    }

    #[test]
    #[serial_test::serial(home_env)]
    fn project_history_keeps_user_receipts_inspect_only() {
        let temp = tempfile::tempdir().unwrap();
        let previous_home = std::env::var("AD_HOME").ok();
        std::env::set_var("AD_HOME", temp.path());
        let installation_id = InstallationId::from("codex:default");
        let receipt = OperationReceipt {
            schema_version: OPERATION_RECEIPT_SCHEMA_VERSION,
            id: ReceiptId::from("user-receipt"),
            plan_id: PlanId::from("user-plan"),
            operation_kind: OperationKind::Apply,
            parent_receipt_id: None,
            context: Some(AgentContext {
                installation_id: installation_id.clone(),
                project_path: None,
            }),
            workspace_key: None,
            action_id: None,
            status: OperationStatus::Complete,
            applied_resources: vec![ResourceRef {
                installation_id: installation_id.clone(),
                project_path: None,
                kind: ResourceKind::Settings,
                scope: ResourceScope::User,
                logical_id: "user-settings".into(),
            }],
            backup_paths: Vec::new(),
            post_apply_states: Vec::new(),
            manifest_digest: Some(ContentDigest::from("sha256:manifest")),
            ownership_changes: Vec::new(),
            ownership_evidence_version: OWNERSHIP_EVIDENCE_VERSION,
            rollback: RollbackEligibility::available(),
            created_at: Some(Utc::now()),
            message: None,
        };
        let state = ExecutionState::open().unwrap();
        state
            .history()
            .write_atomic_new("user-receipt.json", &serde_json::to_vec(&receipt).unwrap())
            .unwrap();

        let history = list_operation_history(
            Some(&installation_id),
            Some("/Users/test/project"),
            Some(20),
        )
        .unwrap();

        assert_eq!(history.len(), 1);
        assert_eq!(
            history[0].receipt.as_ref().unwrap().rollback.reason,
            Some(RollbackUnavailableReason::WorkspaceMismatch)
        );

        match previous_home {
            Some(value) => std::env::set_var("AD_HOME", value),
            None => std::env::remove_var("AD_HOME"),
        }
    }
}
