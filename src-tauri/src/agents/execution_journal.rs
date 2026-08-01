use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[cfg(test)]
use super::execution_state::StateWriteBoundary;
use super::execution_state::{ExecutionState, StateDirectory};
use super::{MutationPlan, PhysicalTargetId, PlanId, ReceiptId, ResourceKind, ResourceRef};

const JOURNAL_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationJournalState {
    Prepared,
    Applying,
    Committed,
    Compensated,
    RepairRequired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationJournalTarget {
    pub id: PhysicalTargetId,
    pub kind: ResourceKind,
    pub logical_id: String,
}

impl From<&ResourceRef> for OperationJournalTarget {
    fn from(resource: &ResourceRef) -> Self {
        Self {
            id: PhysicalTargetId::for_resource(resource),
            kind: resource.kind,
            logical_id: resource.logical_id.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OperationJournal {
    pub schema_version: u32,
    pub instance_id: String,
    pub operation_id: String,
    pub plan_id: PlanId,
    pub planned_receipt_id: ReceiptId,
    pub state: OperationJournalState,
    #[serde(default)]
    pub targets: Vec<OperationJournalTarget>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt_id: Option<ReceiptId>,
}

#[derive(Debug)]
pub(crate) struct OperationJournalHandle {
    directory: StateDirectory,
    name: String,
    journal: OperationJournal,
}

impl OperationJournalHandle {
    pub(super) fn prepare(
        plan: &MutationPlan,
        planned_receipt_id: &ReceiptId,
        instance_id: &str,
        operation_id: &str,
        state: &ExecutionState,
    ) -> Result<Self, std::io::Error> {
        let directory = state.journals().duplicate()?;
        let name = format!("{}.json", operation_fingerprint(operation_id));
        let journal = OperationJournal {
            schema_version: JOURNAL_SCHEMA_VERSION,
            instance_id: instance_id.to_owned(),
            operation_id: operation_id.to_owned(),
            plan_id: plan.id.clone(),
            planned_receipt_id: planned_receipt_id.clone(),
            state: OperationJournalState::Prepared,
            targets: plan
                .mutations
                .iter()
                .map(|mutation| OperationJournalTarget::from(&mutation.resource))
                .collect(),
            receipt_id: None,
        };
        persist_durable(&directory, &name, &journal, false)?;
        Ok(Self {
            directory,
            name,
            journal,
        })
    }

    pub(crate) fn transition(
        &mut self,
        state: OperationJournalState,
        receipt_id: Option<&ReceiptId>,
    ) -> Result<(), std::io::Error> {
        if !valid_transition(self.journal.state, state) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!(
                    "Invalid operation journal transition: {:?} -> {:?}",
                    self.journal.state, state
                ),
            ));
        }
        let previous_state = self.journal.state;
        let previous_receipt = self.journal.receipt_id.clone();
        self.journal.state = state;
        self.journal.receipt_id = receipt_id.cloned();
        if let Err(error) = persist_durable(&self.directory, &self.name, &self.journal, true) {
            self.journal.state = previous_state;
            self.journal.receipt_id = previous_receipt;
            return Err(error);
        }
        Ok(())
    }
}

fn valid_transition(from: OperationJournalState, to: OperationJournalState) -> bool {
    matches!(
        (from, to),
        (
            OperationJournalState::Prepared,
            OperationJournalState::Applying
                | OperationJournalState::Compensated
                | OperationJournalState::RepairRequired
        ) | (
            OperationJournalState::Applying,
            OperationJournalState::Committed
                | OperationJournalState::Compensated
                | OperationJournalState::RepairRequired
        )
    )
}

fn operation_fingerprint(operation_id: &str) -> String {
    format!("{:x}", Sha256::digest(operation_id.as_bytes()))
}

fn persist_durable(
    directory: &StateDirectory,
    name: &str,
    journal: &OperationJournal,
    replace: bool,
) -> Result<(), std::io::Error> {
    let bytes = serde_json::to_vec_pretty(journal)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    if replace {
        directory.write_atomic(name, &bytes)
    } else {
        directory.write_atomic_new(name, &bytes)
    }
}

#[cfg(test)]
fn persist_durable_with(
    directory: &StateDirectory,
    name: &str,
    journal: &OperationJournal,
    replace: bool,
    boundary: impl FnMut(StateWriteBoundary) -> Result<(), std::io::Error>,
) -> Result<(), std::io::Error> {
    let bytes = serde_json::to_vec_pretty(journal)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    directory.write_atomic_with(name, &bytes, replace, boundary)
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, Utc};

    use super::*;
    use crate::agents::{
        AgentContext, AgentId, InstallationId, MutationKind, PlannedMutation, ResourceScope,
    };

    fn plan() -> MutationPlan {
        let resource = ResourceRef {
            installation_id: InstallationId::from("codex:default"),
            project_path: Some("/Users/test/project".into()),
            kind: ResourceKind::Settings,
            scope: ResourceScope::Project,
            logical_id: "project-config".into(),
        };
        MutationPlan {
            id: PlanId::from("plan-journal"),
            agent_id: AgentId::from("codex"),
            context: AgentContext {
                installation_id: resource.installation_id.clone(),
                project_path: resource.project_path.clone(),
            },
            read_set: Vec::new(),
            mutations: vec![PlannedMutation {
                resource,
                kind: MutationKind::Replace,
                expected_digest: None,
                media_type: "application/toml".into(),
                content: Some(serde_json::Value::String("secret = true".into())),
            }],
            expires_at: Utc::now() + Duration::minutes(5),
        }
    }

    fn journal() -> OperationJournal {
        OperationJournal {
            schema_version: JOURNAL_SCHEMA_VERSION,
            instance_id: "instance".into(),
            operation_id: "operation".into(),
            plan_id: PlanId::from("plan"),
            planned_receipt_id: ReceiptId::from("receipt"),
            state: OperationJournalState::Prepared,
            targets: Vec::new(),
            receipt_id: None,
        }
    }

    #[test]
    fn journal_shape_excludes_mutation_content() {
        let journal = OperationJournal {
            targets: plan()
                .mutations
                .iter()
                .map(|mutation| OperationJournalTarget::from(&mutation.resource))
                .collect(),
            ..journal()
        };
        let json = serde_json::to_string(&journal).unwrap();

        assert!(!json.contains("secret"));
        assert!(!json.contains("content"));
        assert!(json.contains("project-config"));
    }

    #[test]
    fn file_sync_failure_does_not_publish_a_journal() {
        let temp = tempfile::tempdir().unwrap();
        let state = ExecutionState::open_at(&temp.path().join(".ad")).unwrap();

        let error = persist_durable_with(
            state.journals(),
            "journal.json",
            &journal(),
            true,
            |boundary| {
                if boundary == StateWriteBoundary::FileSync {
                    return Err(std::io::Error::other("injected file sync failure"));
                }
                Ok(())
            },
        )
        .unwrap_err();

        assert!(error.to_string().contains("file sync"));
        assert!(state.journals().read("journal.json").is_err());
    }

    #[test]
    fn parent_sync_failure_leaves_a_parseable_recovery_record() {
        let temp = tempfile::tempdir().unwrap();
        let state = ExecutionState::open_at(&temp.path().join(".ad")).unwrap();

        let error = persist_durable_with(
            state.journals(),
            "journal.json",
            &journal(),
            true,
            |boundary| {
                if boundary == StateWriteBoundary::ParentSync {
                    return Err(std::io::Error::other("injected parent sync failure"));
                }
                Ok(())
            },
        )
        .unwrap_err();

        assert!(error.to_string().contains("parent sync"));
        let persisted: OperationJournal =
            serde_json::from_slice(&state.journals().read("journal.json").unwrap()).unwrap();
        assert_eq!(persisted.state, OperationJournalState::Prepared);
    }

    #[test]
    fn initial_journal_publish_never_overwrites_an_existing_operation() {
        let temp = tempfile::tempdir().unwrap();
        let state = ExecutionState::open_at(&temp.path().join(".ad")).unwrap();
        let planned_receipt = ReceiptId::from("receipt");
        OperationJournalHandle::prepare(
            &plan(),
            &planned_receipt,
            "first-instance",
            "same-operation",
            &state,
        )
        .unwrap();

        let error = OperationJournalHandle::prepare(
            &plan(),
            &planned_receipt,
            "second-instance",
            "same-operation",
            &state,
        )
        .unwrap_err();

        assert_eq!(error.kind(), std::io::ErrorKind::AlreadyExists);
        let name = format!("{}.json", operation_fingerprint("same-operation"));
        let persisted: OperationJournal =
            serde_json::from_slice(&state.journals().read(&name).unwrap()).unwrap();
        assert_eq!(persisted.instance_id, "first-instance");
    }
}
